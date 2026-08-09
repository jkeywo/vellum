"""The AI-origin audit surface.

Collects everything in a spec that an AI decided and a human has not yet
ratified: entities carrying `origin: ai`, and rationale bullets prefixed with
the literal `[ai]`. Ratification is deletion — a human who audits an item and
finds it correct removes the marker, so this listing is, by construction, the
set of decisions still awaiting review.

Read-only on purpose. The command reports; the human edits; git records who.
"""

from __future__ import annotations

from dataclasses import dataclass

from pasm.core.model import Origin, SpecEntity

# Matched case-insensitively at the start of a rationale bullet. The space is
# not required, so "[AI]" and "[ai] " both count — an inconsistent marker
# should still reach the audit rather than hide from it.
AI_LINE_PREFIX = "[ai]"


@dataclass(frozen=True)
class ReviewItem:
    """One AI-origin item awaiting human audit."""

    entity_id: str
    kind: str
    title: str | None
    # "entity" for an origin: ai entity, "rationale" for a marked bullet.
    scope: str
    # The marked bullet's text, marker included; None for whole entities.
    text: str | None
    location: str


def collect_review_items(entities: tuple[SpecEntity, ...]) -> tuple[ReviewItem, ...]:
    """Every AI-origin item, in entity order, entities before their bullets.

    A marked bullet on an `origin: ai` entity is reported once, as the entity
    — the whole entity is already awaiting audit, and listing its bullets
    separately would double-count the same review.
    """
    items: list[ReviewItem] = []
    for entity in entities:
        if entity.origin is Origin.AI:
            items.append(
                ReviewItem(
                    entity_id=entity.id.value,
                    kind=entity.kind,
                    title=entity.title,
                    scope="entity",
                    text=None,
                    location=entity.source_location.render(),
                )
            )
            continue
        for line in entity.rationale:
            if line.lstrip().lower().startswith(AI_LINE_PREFIX):
                items.append(
                    ReviewItem(
                        entity_id=entity.id.value,
                        kind=entity.kind,
                        title=entity.title,
                        scope="rationale",
                        text=line,
                        location=entity.source_location.render(),
                    )
                )
    return tuple(items)


def review_to_json(items: tuple[ReviewItem, ...]) -> dict:
    return {
        "schema_version": 1,
        "items": [
            {
                "entity_id": item.entity_id,
                "kind": item.kind,
                "title": item.title,
                "scope": item.scope,
                "text": item.text,
                "location": item.location,
            }
            for item in items
        ],
        "entity_count": sum(1 for item in items if item.scope == "entity"),
        "rationale_count": sum(1 for item in items if item.scope == "rationale"),
        "ratification": "Delete the marker (`origin: ai` or the `[ai] ` prefix) to ratify an audited item as human-decided.",
    }


def review_to_text(items: tuple[ReviewItem, ...]) -> str:
    if not items:
        return "No AI-origin items awaiting audit."
    lines = ["AI-origin items awaiting audit:", ""]
    for item in items:
        title = f" — {item.title}" if item.title else ""
        lines.append(f"{item.entity_id} ({item.kind}){title}")
        lines.append(f"  at {item.location}")
        if item.scope == "rationale":
            text = item.text or ""
            if len(text) > 160:
                text = text[:157] + "..."
            lines.append(f"  rationale: {text}")
        lines.append("")
    entity_count = sum(1 for item in items if item.scope == "entity")
    rationale_count = len(items) - entity_count
    lines.append(f"{entity_count} entities, {rationale_count} rationale lines.")
    lines.append("Ratify by deleting the marker; the audit lists only what remains.")
    return "\n".join(lines)
