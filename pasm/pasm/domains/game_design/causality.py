"""Causal-structure validators for the game-design dynamics vocabulary.

The causal spine is: gates progress the scenario (`requires`/`enables`),
player verbs and gates produce outcomes (`on_success`/`on_failure`), and
design-significant outcomes are `consequence` entities. These checks make
silent causality impossible: a gate that resolves without player input, a
deadline whose failure means nothing, and a consequence nothing causes are
all findings.
"""

from __future__ import annotations

from pasm.core.findings import Finding, FindingCategory, Severity
from pasm.core.model import EntityId, SpecEntity, Status


# Incomplete is allowed when declared, broken is not: a draft entity's missing
# causal intent is a warning to fill in, not a broken model.
_DRAFT_STATUSES = {Status.PROPOSED, Status.PROVISIONAL}


KERNEL_KINDS = {
    "gate",
    "consequence",
    "scenario_model",
    "scenario-model",
    "pacing",
    "design_principle",
    "design-principle",
    "design_invariant",
    "design-invariant",
}

_CAUSAL_SOURCE_FIELDS = ("on_success", "on_failure")


def effective_kind(entity: SpecEntity) -> str:
    design = entity.game_design
    if design is not None and design.specialises in KERNEL_KINDS:
        return design.specialises.replace("-", "_")
    return entity.kind.replace("-", "_")


def validate_causality(entities: tuple[SpecEntity, ...]) -> list[Finding]:
    findings: list[Finding] = []
    index = {entity.id: entity for entity in entities}
    findings.extend(_validate_specialises(entities))
    findings.extend(_validate_gates(entities))
    findings.extend(_validate_gate_cycles(entities, index))
    findings.extend(_validate_consequences(entities, index))
    findings.extend(_validate_design_invariants(entities))
    return findings


def _validate_design_invariants(entities):
    findings = []
    for entity in entities:
        if effective_kind(entity) != "design_invariant" or entity.game_design is None:
            continue
        design = entity.game_design
        for field_name, missing_summary, details, resolution in (
            ("relation", "states no relation", "An invariant must state the relation over authored numbers it protects.", "Add 'game_design.relation'."),
            ("anchors", "anchors no authored values", "An invariant's numbers live in the authored content; anchors say where.", "Add 'game_design.anchors' for each number the relation names."),
            ("asserted_by", "names no asserting test", "PASM never evaluates the relation — the named test does. Without one, the invariant is prose.", "Add 'game_design.asserted_by' with the asserting test name(s)."),
        ):
            if getattr(design, field_name):
                continue
            findings.append(_finding(
                f"invariant-missing-{field_name.replace('_', '-')}:{entity.id}", entity,
                f"Design invariant '{entity.id}' {missing_summary}.",
                details,
                f"game-design.invariant-{field_name.replace('_', '-')}-required",
                resolution,
                _location(entity, field_name),
            ))
    return findings


def _validate_specialises(entities):
    findings = []
    for entity in entities:
        design = entity.game_design
        if design is None or design.specialises is None:
            continue
        if design.specialises not in KERNEL_KINDS:
            findings.append(_finding(
                f"specialises-unknown-kernel-kind:{entity.id}:{design.specialises}", entity,
                f"Entity '{entity.id}' specialises unknown kernel kind '{design.specialises}'.",
                "Game-local kinds may specialise only a kernel design kind so its validators apply.",
                "game-design.specialises-kernel-kind",
                "Point 'specialises' at a kernel kind such as 'gate' or 'consequence'.",
                _location(entity, "specialises"),
            ))
    return findings


def _validate_gates(entities):
    findings = []
    for entity in entities:
        if effective_kind(entity) != "gate" or entity.game_design is None:
            continue
        design = entity.game_design
        if not design.requires_player_action and design.self_resolving is not True:
            findings.append(_finding(
                f"gate-zero-input:{entity.id}", entity,
                f"Gate '{entity.id}' resolves with no player action and is not declared self-resolving.",
                "A gate that requires no player verb and does not declare 'self_resolving: true' "
                "advances the scenario while the crew does nothing — the designed act may be a cutscene.",
                "game-design.gate-zero-input",
                "Add 'requires_player_action' verbs, or declare 'self_resolving: true' if intentional.",
                _location(entity, "requires_player_action", "self_resolving"),
                severity=Severity.WARNING,
                category=FindingCategory.DESIGN_RISK,
            ))
        if design.deadline_id is not None and not design.on_failure and design.benign is not True:
            draft = entity.status in _DRAFT_STATUSES
            findings.append(_finding(
                f"deadline-consequence-missing:{entity.id}", entity,
                f"Deadline gate '{entity.id}' declares no failure consequence and is not marked benign.",
                "Every deadline must state what failing it causes, or declare 'benign: true' so the "
                "absence of a consequence is a decision rather than an omission.",
                "game-design.deadline-consequence-required",
                "Add 'on_failure' consequences or 'benign: true'.",
                _location(entity, "deadline_id"),
                severity=Severity.WARNING if draft else Severity.ERROR,
                category=FindingCategory.UNVERIFIED if draft else FindingCategory.VIOLATION,
            ))
    return findings


def _validate_gate_cycles(entities, index):
    # Edge A -> B when A requires B, or B enables A: B must resolve before A.
    edges: dict[EntityId, set[EntityId]] = {}
    for entity in entities:
        if effective_kind(entity) != "gate" or entity.game_design is None:
            continue
        edges.setdefault(entity.id, set()).update(
            target for target in entity.game_design.requires
            if target in index and effective_kind(index[target]) == "gate"
        )
        for target in entity.game_design.enables:
            if target in index and effective_kind(index[target]) == "gate":
                edges.setdefault(target, set()).add(entity.id)

    findings = []
    state: dict[EntityId, int] = {}

    def visit(node: EntityId, path: list[EntityId]) -> list[EntityId] | None:
        state[node] = 1
        for successor in sorted(edges.get(node, ()), key=lambda item: item.value):
            if state.get(successor) == 1:
                return path + [node, successor]
            if state.get(successor, 0) == 0:
                cycle = visit(successor, path + [node])
                if cycle is not None:
                    return cycle
        state[node] = 2
        return None

    for node in sorted(edges, key=lambda item: item.value):
        if state.get(node, 0) != 0:
            continue
        cycle = visit(node, [])
        if cycle is not None:
            chain = " -> ".join(item.value for item in cycle)
            entity = index[cycle[0]]
            findings.append(_finding(
                f"gate-cycle:{cycle[0]}", entity,
                f"Gate dependency cycle: {chain}.",
                "A cycle in requires/enables means no gate in it can ever resolve first.",
                "game-design.gate-cycle-free",
                "Break the cycle by removing or redirecting one dependency.",
                entity.source_location,
            ))
            break
    return findings


def _validate_consequences(entities, index):
    findings = []
    caused: dict[EntityId, list[SpecEntity]] = {}
    for entity in entities:
        design = entity.game_design
        if design is None:
            continue
        for field_name in _CAUSAL_SOURCE_FIELDS:
            for target in getattr(design, field_name):
                caused.setdefault(target, []).append(entity)

    for entity in entities:
        if effective_kind(entity) != "consequence" or entity.game_design is None:
            continue
        design = entity.game_design
        causers = caused.get(entity.id, [])
        if not causers:
            draft = entity.status in _DRAFT_STATUSES
            findings.append(_finding(
                f"consequence-orphaned:{entity.id}", entity,
                f"Consequence '{entity.id}' is caused by nothing.",
                "No entity names this consequence in 'on_success' or 'on_failure', so the model "
                "claims an outcome the design never produces.",
                "game-design.consequence-caused",
                "Reference this consequence from the gate or verb that produces it, or remove it.",
                entity.source_location,
                severity=Severity.WARNING if draft else Severity.ERROR,
                category=FindingCategory.UNVERIFIED if draft else FindingCategory.VIOLATION,
            ))
        for state_ref in design.depends_on_state:
            state_entity = index.get(state_ref)
            if state_entity is None or state_entity.game_design is None:
                continue
            viewers = state_entity.game_design.permitted_viewers
            if not viewers:
                continue
            causing_roles = {
                causer.game_design.owner_role
                for causer in causers
                if causer.game_design is not None and causer.game_design.owner_role is not None
            }
            unsighted = causing_roles - set(viewers)
            if causing_roles and unsighted:
                names = ", ".join(sorted(role.value for role in unsighted))
                findings.append(_finding(
                    f"consequence-state-not-visible:{entity.id}:{state_ref}", entity,
                    f"Consequence '{entity.id}' depends on state '{state_ref}' its causing role(s) cannot see ({names}).",
                    "The consequence's magnitude reads game state the deciding role has no view of, "
                    "so the player cannot weigh the outcome they are choosing.",
                    "game-design.consequence-state-visible",
                    "Permit the causing role to view the state, or record why the blindness is intended.",
                    _location(entity, "depends_on_state"),
                    severity=Severity.WARNING,
                    category=FindingCategory.DESIGN_RISK,
                ))
    return findings


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
