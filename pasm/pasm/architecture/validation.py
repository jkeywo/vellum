from __future__ import annotations

from pasm.core.findings import Finding, FindingCategory, Severity
from pasm.core.model import EntityId, SpecEntity
from pasm.relations import DEPENDENCY_FIELDS, build_graph, cyclic_components, shortest_path


def validate_architecture(entities: tuple[SpecEntity, ...]) -> list[Finding]:
    findings: list[Finding] = []
    index = {entity.id: entity for entity in entities}
    graph = build_graph(entities)
    findings.extend(_validate_architecture_reference_resolution(entities, index))
    findings.extend(_validate_authoritative_state_ownership(entities, index))
    findings.extend(_validate_forbidden_dependencies(entities))
    findings.extend(_validate_dependency_policy(entities))
    findings.extend(_validate_indirect_forbidden_dependencies(entities, graph))
    findings.extend(_validate_dependency_cycles(entities, index, graph))
    findings.extend(_validate_message_contracts(entities))
    return findings


def _validate_architecture_reference_resolution(
    entities: tuple[SpecEntity, ...],
    index: dict[EntityId, SpecEntity],
) -> list[Finding]:
    findings: list[Finding] = []
    for entity in entities:
        architecture = entity.architecture
        if architecture is None:
            continue
        for target in _architecture_refs(architecture):
            if target not in index:
                findings.append(
                    Finding(
                        id=f"unknown-architecture-reference:{entity.id}:{target}",
                        category=FindingCategory.VIOLATION,
                        severity=Severity.ERROR,
                        confidence="confirmed",
                        summary=f"Entity '{entity.id}' references unknown architecture entity '{target}'.",
                        details="Architecture relationships must resolve after all PASM files are loaded.",
                        rule="architecture.reference-target-exists",
                        spec_entities=(entity.id,),
                        implementation_locations=(entity.source_location,),
                        evidence=(),
                        suggested_resolution="Create the referenced entity or remove the unresolved architecture link.",
                        requires_decision=False,
                    )
                )
    return findings


def _validate_authoritative_state_ownership(
    entities: tuple[SpecEntity, ...],
    index: dict[EntityId, SpecEntity],
) -> list[Finding]:
    findings: list[Finding] = []
    owners_by_state: dict[EntityId, list[SpecEntity]] = {}

    for entity in entities:
        architecture = entity.architecture
        if architecture is None:
            continue
        for owned in architecture.owns:
            owners_by_state.setdefault(owned, []).append(entity)

    for entity in entities:
        architecture = entity.architecture
        if architecture is None or entity.kind != "state":
            continue
        if architecture.classification != "authoritative":
            continue
        if architecture.owner is None:
            findings.append(
                Finding(
                    id=f"authoritative-state-missing-owner:{entity.id}",
                    category=FindingCategory.ARCHITECTURE_RISK,
                    severity=Severity.ERROR,
                    confidence="confirmed",
                    summary=f"Authoritative state '{entity.id}' has no declared owner.",
                    details="Phase 3 requires every authoritative state to declare one owning entity.",
                    rule="architecture.authoritative-state-owner-required",
                    spec_entities=(entity.id,),
                    implementation_locations=(entity.source_location,),
                    evidence=(),
                    suggested_resolution="Add 'architecture.owner' and mirror the relationship in the owning entity's 'owns' list.",
                    requires_decision=False,
                )
            )
            continue

        owner_entity = index.get(architecture.owner)
        if owner_entity is None:
            continue

        declared_owners = owners_by_state.get(entity.id, [])
        if not any(candidate.id == architecture.owner for candidate in declared_owners):
            findings.append(
                Finding(
                    id=f"authoritative-state-owner-not-mirrored:{entity.id}:{architecture.owner}",
                    category=FindingCategory.INCOMPLETE_MIGRATION,
                    severity=Severity.ERROR,
                    confidence="confirmed",
                    summary=f"Authoritative state '{entity.id}' names owner '{architecture.owner}' but that owner does not list the state in 'owns'.",
                    details="The state-owner relationship should be explicit from both directions so ownership checks stay deterministic.",
                    rule="architecture.owner-mirrors-state",
                    spec_entities=(entity.id, architecture.owner),
                    implementation_locations=(entity.source_location, owner_entity.source_location),
                    evidence=(),
                    suggested_resolution="Add the state to the owner's 'architecture.owns' list or correct the state's 'owner' field.",
                    requires_decision=False,
                )
            )

        if len(declared_owners) > 1:
            findings.append(
                Finding(
                    id=f"duplicate-authoritative-ownership:{entity.id}",
                    category=FindingCategory.ARCHITECTURE_RISK,
                    severity=Severity.ERROR,
                    confidence="confirmed",
                    summary=f"Authoritative state '{entity.id}' is owned by multiple entities.",
                    details="Exactly one entity should own an authoritative state in the authored architecture model.",
                    rule="architecture.single-authoritative-owner",
                    spec_entities=tuple(owner.id for owner in declared_owners) + (entity.id,),
                    implementation_locations=tuple(owner.source_location for owner in declared_owners) + (entity.source_location,),
                    evidence=(),
                    suggested_resolution="Leave the state in only one entity's 'owns' list.",
                    requires_decision=False,
                )
            )

        owner_architecture = owner_entity.architecture
        if owner_architecture is not None and owner_architecture.authority == "non-authoritative":
            findings.append(
                Finding(
                    id=f"non-authoritative-owner:{entity.id}:{owner_entity.id}",
                    category=FindingCategory.ARCHITECTURE_RISK,
                    severity=Severity.ERROR,
                    confidence="confirmed",
                    summary=f"Non-authoritative entity '{owner_entity.id}' owns authoritative state '{entity.id}'.",
                    details="Client or otherwise non-authoritative entities must not own authoritative state.",
                    rule="architecture.non-authoritative-cannot-own-authoritative-state",
                    spec_entities=(owner_entity.id, entity.id),
                    implementation_locations=(owner_entity.source_location, entity.source_location),
                    evidence=(),
                    suggested_resolution="Move ownership to an authoritative entity or change the entity authority classification if the model is wrong.",
                    requires_decision=False,
                )
            )

    return findings


def _validate_forbidden_dependencies(entities: tuple[SpecEntity, ...]) -> list[Finding]:
    findings: list[Finding] = []
    for entity in entities:
        architecture = entity.architecture
        if architecture is None or not architecture.must_not_depend_on:
            continue
        forbidden = set(architecture.must_not_depend_on)
        overlaps: set[EntityId] = set()
        for field_name in DEPENDENCY_FIELDS:
            overlaps.update(set(getattr(architecture, field_name)) & forbidden)
        for overlap in sorted(overlaps, key=lambda value: value.value):
            findings.append(
                Finding(
                    id=f"forbidden-dependency:{entity.id}:{overlap}",
                    category=FindingCategory.VIOLATION,
                    severity=Severity.ERROR,
                    confidence="confirmed",
                    summary=f"Entity '{entity.id}' both depends on and forbids dependency on '{overlap}'.",
                    details="A forbidden dependency cannot also appear in the entity's declared dependency lists.",
                    rule="architecture.forbidden-dependency",
                    spec_entities=(entity.id, overlap),
                    implementation_locations=(entity.source_location,),
                    evidence=(),
                    suggested_resolution="Remove the forbidden dependency from the dependency list or from 'must_not_depend_on'.",
                    requires_decision=False,
                )
            )
    return findings


DEPENDENCY_POLICIES = ("open", "closed")


def _validate_dependency_policy(entities: tuple[SpecEntity, ...]) -> list[Finding]:
    findings: list[Finding] = []
    for entity in entities:
        architecture = entity.architecture
        if architecture is None or architecture.dependency_policy is None:
            continue
        if architecture.dependency_policy in DEPENDENCY_POLICIES:
            continue
        findings.append(
            Finding(
                id=f"invalid-dependency-policy:{entity.id}",
                category=FindingCategory.VIOLATION,
                severity=Severity.ERROR,
                confidence="confirmed",
                summary=(
                    f"Entity '{entity.id}' declares unknown dependency policy "
                    f"'{architecture.dependency_policy}'."
                ),
                details="A dependency policy is 'open' (the default) or 'closed'.",
                rule="architecture.dependency-policy-vocabulary",
                spec_entities=(entity.id,),
                implementation_locations=(entity.source_location,),
                evidence=DEPENDENCY_POLICIES,
                suggested_resolution="Use 'open' or 'closed', or remove the field.",
                requires_decision=False,
            )
        )
    return findings


def _validate_indirect_forbidden_dependencies(
    entities: tuple[SpecEntity, ...],
    graph: dict[EntityId, tuple[EntityId, ...]],
) -> list[Finding]:
    findings: list[Finding] = []
    for entity in entities:
        architecture = entity.architecture
        if architecture is None or not architecture.must_not_depend_on:
            continue
        direct = set()
        for field_name in DEPENDENCY_FIELDS:
            direct.update(getattr(architecture, field_name))
        for target in sorted(set(architecture.must_not_depend_on), key=lambda value: value.value):
            if target in direct:
                continue  # Already reported as a direct forbidden dependency.
            path = shortest_path(graph, entity.id, target)
            if not path:
                continue
            findings.append(
                Finding(
                    id=f"indirect-forbidden-dependency:{entity.id}:{target}",
                    category=FindingCategory.PROBABLE_VIOLATION,
                    severity=Severity.WARNING,
                    confidence="inferred",
                    summary=f"Entity '{entity.id}' reaches forbidden dependency '{target}' through declared intermediaries.",
                    details=(
                        "The declared dependency graph has a path to an entity this one forbids "
                        "depending on. The path may cross a legitimate interface, so PASM reports "
                        "it for a decision rather than asserting a violation."
                    ),
                    rule="architecture.indirect-forbidden-dependency",
                    spec_entities=(entity.id, target),
                    implementation_locations=(entity.source_location,),
                    evidence=(" -> ".join(step.value for step in path),),
                    suggested_resolution=(
                        "Break the path, narrow the rule to direct dependencies, or record an "
                        "exception naming the intermediary that isolates the two."
                    ),
                    requires_decision=True,
                )
            )
    return findings


def _validate_dependency_cycles(
    entities: tuple[SpecEntity, ...],
    index: dict[EntityId, SpecEntity],
    graph: dict[EntityId, tuple[EntityId, ...]],
) -> list[Finding]:
    findings: list[Finding] = []
    for members in cyclic_components(graph):
        first = members[0]
        path = shortest_path(graph, first, first)
        findings.append(
            Finding(
                id=f"dependency-cycle:{first}",
                category=FindingCategory.ARCHITECTURE_RISK,
                severity=Severity.WARNING,
                confidence="confirmed",
                summary=(
                    f"Entity '{first}' is in a declared dependency cycle with "
                    f"{len(members) - 1} other entit{'y' if len(members) == 2 else 'ies'}."
                    if len(members) > 1
                    else f"Entity '{first}' declares a dependency on itself."
                ),
                details=(
                    "These entities are mutually reachable through declared dependencies. A cycle "
                    "is not always wrong, but it means neither entity can be understood, changed, "
                    "or extracted without the other."
                ),
                rule="architecture.dependency-cycle",
                spec_entities=members,
                implementation_locations=tuple(
                    index[member].source_location for member in members if member in index
                ),
                evidence=((" -> ".join(step.value for step in path),) if path else ())
                + tuple(member.value for member in members),
                suggested_resolution="Break the cycle, or invert one edge through an interface entity that both sides depend on.",
                requires_decision=True,
            )
        )
    return findings


def _validate_message_contracts(entities: tuple[SpecEntity, ...]) -> list[Finding]:
    findings: list[Finding] = []
    for entity in entities:
        architecture = entity.architecture
        if architecture is None or entity.kind != "message":
            continue
        if not architecture.producer:
            findings.append(
                Finding(
                    id=f"message-missing-producer:{entity.id}",
                    category=FindingCategory.UNIMPLEMENTED_SPECIFICATION,
                    severity=Severity.ERROR,
                    confidence="confirmed",
                    summary=f"Message '{entity.id}' has no producer.",
                    details="Phase 3 message entities should identify at least one producer.",
                    rule="architecture.message-producer-required",
                    spec_entities=(entity.id,),
                    implementation_locations=(entity.source_location,),
                    evidence=(),
                    suggested_resolution="Add one or more entities to 'architecture.producer'.",
                    requires_decision=False,
                )
            )
        if not architecture.consumer:
            findings.append(
                Finding(
                    id=f"message-missing-consumer:{entity.id}",
                    category=FindingCategory.UNIMPLEMENTED_SPECIFICATION,
                    severity=Severity.ERROR,
                    confidence="confirmed",
                    summary=f"Message '{entity.id}' has no consumer.",
                    details="Phase 3 message entities should identify at least one consumer.",
                    rule="architecture.message-consumer-required",
                    spec_entities=(entity.id,),
                    implementation_locations=(entity.source_location,),
                    evidence=(),
                    suggested_resolution="Add one or more entities to 'architecture.consumer'.",
                    requires_decision=False,
                )
            )
        if architecture.trust_boundary and not architecture.validator:
            findings.append(
                Finding(
                    id=f"trust-boundary-message-missing-validator:{entity.id}",
                    category=FindingCategory.ARCHITECTURE_RISK,
                    severity=Severity.ERROR,
                    confidence="confirmed",
                    summary=f"Trust-boundary message '{entity.id}' has no validator.",
                    details="Messages that cross a trust boundary should identify a validation surface in the authored architecture model.",
                    rule="architecture.trust-boundary-validator-required",
                    spec_entities=(entity.id,),
                    implementation_locations=(entity.source_location,),
                    evidence=(),
                    suggested_resolution="Add one or more validator entities to 'architecture.validator'.",
                    requires_decision=False,
                )
            )
    return findings


def _architecture_refs(architecture) -> tuple[EntityId, ...]:
    refs: list[EntityId] = []
    for field_name in (
        "owner",
        "replacement",
    ):
        value = getattr(architecture, field_name)
        if value is not None:
            refs.append(value)
    for field_name in (
        "owns",
        "reads",
        "readers",
        "writes",
        "produces",
        "consumes",
        "exposes",
        "receives",
        "transforms",
        "coordinates",
        "validates",
        "persists",
        "renders",
        "accepts",
        "sends",
        "depends_on",
        "may_depend_on",
        "must_not_depend_on",
        "runtime_depends_on",
        "build_depends_on",
        "optional_dependency",
        "temporary_dependency",
        "writers",
        "replicas",
        "derived_from",
        "reveal_conditions",
        "runs_in",
        "producer",
        "consumer",
        "validator",
    ):
        refs.extend(getattr(architecture, field_name))
    return tuple(refs)
