from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum

class InformationVisibility(str, Enum):
    PUBLIC = "public"
    ROLE_VISIBLE = "role-visible"
    TEAM_VISIBLE = "team-visible"
    HIDDEN = "hidden"
    PARTIALLY_KNOWN = "partially-known"
    DERIVED = "derived"
    DELAYED = "delayed"
    UNCERTAIN = "uncertain"


@dataclass(frozen=True)
class ContentAnchor:
    """A claim about where an authored value lives, and what the design intends of it.

    All expectation values are strings; the content validator types them at
    check time. Bounds (`min`/`max`) are design intent a human may retune past
    (a warning-level conversation); `expect`/`expect_count` are structural
    claims whose drift is an error.
    """

    name: str | None = None
    path: str | None = None
    table: str | None = None
    match: str | None = None
    key: str | None = None
    expect: str | None = None
    min: str | None = None
    max: str | None = None
    expect_count: str | None = None
    aggregate: str | None = None
    source_location: object | None = None


@dataclass(frozen=True)
class PacingPhase:
    """One window of the mission clock. `start`/`end` are quoted durations
    ("40s", "5m", "300"), typed by the pacing validator at check time."""

    phase_id: str | None = None
    start: str | None = None
    end: str | None = None
    intensity_intent: str | None = None
    engaged_roles: tuple[EntityId, ...] = ()
    covers_deadlines: tuple[str, ...] = ()
    source_location: object | None = None


@dataclass(frozen=True)
class GameDesignSection:
    field_locations: dict[str, object] = field(default_factory=dict)
    architecture_links: tuple[EntityId, ...] = ()
    enforcement_links: tuple[EntityId, ...] = ()
    responsibilities: tuple[str, ...] = ()
    player_verbs: tuple[EntityId, ...] = ()
    exclusive_verbs: tuple[EntityId, ...] = ()
    protected_decisions: tuple[EntityId, ...] = ()
    visible_information: tuple[EntityId, ...] = ()
    hidden_information: tuple[EntityId, ...] = ()
    coordination_with: tuple[EntityId, ...] = ()
    expected_decision_frequency: str | None = None
    owner_role: EntityId | None = None
    protected: bool | None = None
    must_not_be: tuple[str, ...] = ()
    visibility: InformationVisibility | None = None
    permitted_viewers: tuple[EntityId, ...] = ()
    reveal_conditions: tuple[str, ...] = ()
    indirect_signals: tuple[str, ...] = ()
    architectural_enforcement: tuple[str, ...] = ()
    participating_roles: tuple[EntityId, ...] = ()
    inputs: tuple[str, ...] = ()
    reads: tuple[EntityId, ...] = ()
    changes: tuple[EntityId, ...] = ()
    eligibility: tuple[str, ...] = ()
    costs: tuple[str, ...] = ()
    resolution: str | None = None
    outputs: tuple[str, ...] = ()
    produces_facts: tuple[str, ...] = ()
    failure: tuple[EntityId, ...] = ()
    side_effects: tuple[str, ...] = ()
    information_revealed: tuple[EntityId, ...] = ()
    information_exchanged: tuple[EntityId, ...] = ()
    actions_required: tuple[EntityId, ...] = ()
    intended_player_effect: str | None = None
    implementation_path: tuple[str, ...] = ()
    sources: tuple[str, ...] = ()
    sinks: tuple[str, ...] = ()
    capacity: str | None = None
    pressure_intent: tuple[str, ...] = ()
    causes: tuple[str, ...] = ()
    consequences: tuple[str, ...] = ()
    affected_roles: tuple[EntityId, ...] = ()
    visible_to: tuple[EntityId, ...] = ()
    terminal: bool | None = None
    recovery_paths: tuple[str, ...] = ()
    affected_mechanics: tuple[EntityId, ...] = ()
    intended_directional_effect: str | None = None
    bounds: str | None = None
    maturity: str | None = None
    supporting_evidence: tuple[str, ...] = ()
    claim: str | None = None
    supports: tuple[EntityId, ...] = ()
    specialises: str | None = None
    requires: tuple[EntityId, ...] = ()
    enables: tuple[EntityId, ...] = ()
    requires_player_action: tuple[EntityId, ...] = ()
    self_resolving: bool | None = None
    teaches: tuple[EntityId, ...] = ()
    on_success: tuple[EntityId, ...] = ()
    on_failure: tuple[EntityId, ...] = ()
    benign: bool | None = None
    deadline_id: str | None = None
    magnitude_source: str | None = None
    depends_on_state: tuple[EntityId, ...] = ()
    campaign_flags: tuple[str, ...] = ()
    handler: str | None = None
    severity_intent: str | None = None
    world_file: str | None = None
    deadline_table: str | None = None
    deadline_id_key: str | None = None
    anchors: tuple[ContentAnchor, ...] = ()
    relation: str | None = None
    asserted_by: tuple[str, ...] = ()
    phases: tuple[PacingPhase, ...] = ()
    clock_source: str | None = None
    human_calibration: str | None = None
    deadline_due_key: str | None = None
    context: str | None = None
    construction: tuple[EntityId, ...] = ()
    expected_dynamic: str | None = None
    experience_hypothesis: str | None = None
    measured_by: tuple[str, ...] = ()
    strength: str | None = None
    counter_evidence: tuple[str, ...] = ()
