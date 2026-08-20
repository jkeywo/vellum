"""The design digest: one derived view an LLM reads instead of the world file.

Combines declared design intent (gates, consequences, invariants) with live
values read from the authoritative content, plus the content hashes a
write-back must present. Deterministic output: same spec + same content =
same digest.
"""

from __future__ import annotations

import hashlib
from pathlib import Path

from pasm.core.model import SpecEntity
from pasm.domains.game_design.causality import effective_kind
from pasm.domains.game_design.content import DEFAULT_DEADLINE_ID_KEY, DEFAULT_DEADLINE_TABLE
from pasm.scanners.content import ContentDocument, ContentParseError, extract_content


def build_design_digest(entities: tuple[SpecEntity, ...], workspace_root: Path) -> dict:
    """Build the digest as plain data; the CLI renders text or JSON from it."""
    scenario_models = _of_kind(entities, "scenario_model")
    gates = _of_kind(entities, "gate")
    consequences = {entity.id: entity for entity in _of_kind(entities, "consequence")}
    invariants = _of_kind(entities, "design_invariant")

    documents: dict[str, ContentDocument] = {}
    files: list[dict] = []
    deadline_rows: dict[str, dict] = {}
    for model in scenario_models:
        design = model.game_design
        if design.world_file is None:
            continue
        content_path = (workspace_root / design.world_file).resolve()
        entry: dict = {"scenario_model": model.id.value, "world_file": design.world_file}
        if not content_path.is_file():
            entry["error"] = "missing"
            files.append(entry)
            continue
        # newline='' so the hash matches the bytes writeback will guard against.
        with open(content_path, "r", encoding="utf-8", newline="") as handle:
            text = handle.read()
        entry["sha256"] = hashlib.sha256(text.encode("utf-8")).hexdigest()
        try:
            document = extract_content(text, content_path.suffix)
        except ContentParseError as exc:
            entry["error"] = str(exc)
            files.append(entry)
            continue
        documents[design.world_file] = document
        files.append(entry)
        table = design.deadline_table or DEFAULT_DEADLINE_TABLE
        id_key = design.deadline_id_key or DEFAULT_DEADLINE_ID_KEY
        for row in document.rows(table):
            row_id = row.values.get(id_key)
            if isinstance(row_id, str):
                deadline_rows[row_id] = dict(row.values)

    gate_entries = []
    for gate in gates:
        design = gate.game_design
        entry = {
            "id": gate.id.value,
            "status": gate.status.value,
            "title": gate.title,
            "requires": [item.value for item in design.requires],
            "enables": [item.value for item in design.enables],
            "requires_player_action": [item.value for item in design.requires_player_action],
            "self_resolving": design.self_resolving,
            "benign": design.benign,
            "on_success": [_consequence_entry(consequences.get(item), item) for item in design.on_success],
            "on_failure": [_consequence_entry(consequences.get(item), item) for item in design.on_failure],
        }
        if design.deadline_id is not None:
            entry["deadline_id"] = design.deadline_id
            entry["authored"] = deadline_rows.get(design.deadline_id)
        gate_entries.append(entry)

    anchor_entries = []
    for entity in entities:
        design = entity.game_design
        if design is None:
            continue
        for anchor in design.anchors:
            item = {
                "entity": entity.id.value,
                "name": anchor.name,
                "path": anchor.path,
                "table": anchor.table,
                "match": anchor.match,
                "key": anchor.key,
                "expect": anchor.expect,
                "min": anchor.min,
                "max": anchor.max,
                "aggregate": anchor.aggregate,
            }
            document = documents.get(anchor.path) if anchor.path else None
            if document is None and anchor.path is not None:
                content_path = (workspace_root / anchor.path).resolve()
                if content_path.is_file():
                    try:
                        document = extract_content(content_path.read_text(encoding="utf-8"), content_path.suffix)
                        documents[anchor.path] = document
                    except ContentParseError:
                        document = None
            if document is not None and anchor.table is not None:
                resolved = document.resolve(anchor.table, anchor.match, anchor.key)
                values = [entry.value for entry in resolved]
                if anchor.aggregate in {"sum", "min", "max"}:
                    numeric = [v for v in values if isinstance(v, (int, float)) and not isinstance(v, bool)]
                    if numeric:
                        values = [{"sum": sum(numeric), "min": min(numeric), "max": max(numeric)}[anchor.aggregate]]
                item["live"] = values
            anchor_entries.append(item)

    invariant_entries = [
        {
            "id": entity.id.value,
            "status": entity.status.value,
            "relation": entity.game_design.relation,
            "asserted_by": list(entity.game_design.asserted_by),
        }
        for entity in invariants
    ]

    return {
        "files": files,
        "gates": gate_entries,
        "anchors": anchor_entries,
        "invariants": invariant_entries,
    }


def render_design_digest(digest: dict) -> str:
    lines: list[str] = ["# Design digest", ""]

    if digest["files"]:
        lines.append("## Anchored content (hashes guard write-back)")
        for entry in digest["files"]:
            marker = entry.get("error") or entry.get("sha256", "")
            lines.append(f"- {entry['world_file']} ({entry['scenario_model']}): {marker}")
        lines.append("")

    if digest["gates"]:
        lines.append("## Causal chain")
        for gate in digest["gates"]:
            headline = f"### {gate['id']} [{gate['status']}]"
            lines.append(headline)
            if gate.get("deadline_id"):
                authored = gate.get("authored")
                lines.append(f"- deadline: {gate['deadline_id']} authored={authored}")
            if gate["requires"]:
                lines.append(f"- requires: {', '.join(gate['requires'])}")
            if gate["enables"]:
                lines.append(f"- enables: {', '.join(gate['enables'])}")
            if gate["requires_player_action"]:
                lines.append(f"- player action: {', '.join(gate['requires_player_action'])}")
            elif gate["self_resolving"]:
                lines.append("- self-resolving (no player action by design)")
            for label, outcomes in (("on success", gate["on_success"]), ("on failure", gate["on_failure"])):
                for outcome in outcomes:
                    lines.append(f"- {label} -> {_render_consequence(outcome)}")
            if gate.get("benign"):
                lines.append("- failing this deadline is declared benign")
            lines.append("")

    if digest["anchors"]:
        lines.append("## Anchored values (live from content)")
        for anchor in digest["anchors"]:
            name = anchor["name"] or f"{anchor['table']}.{anchor['key']}"
            bounds = []
            if anchor["min"] is not None:
                bounds.append(f"min {anchor['min']}")
            if anchor["max"] is not None:
                bounds.append(f"max {anchor['max']}")
            if anchor["expect"] is not None:
                bounds.append(f"expect {anchor['expect']}")
            suffix = f" [{', '.join(bounds)}]" if bounds else ""
            live = anchor.get("live", "unresolved")
            lines.append(f"- {anchor['entity']}.{name}: {live}{suffix}")
        lines.append("")

    if digest["invariants"]:
        lines.append("## Invariants")
        for invariant in digest["invariants"]:
            lines.append(f"- {invariant['id']} [{invariant['status']}]: {invariant['relation']}")
            for test in invariant["asserted_by"]:
                lines.append(f"  asserted by: {test}")
        lines.append("")

    return "\n".join(lines)


def _of_kind(entities, kind: str):
    return [
        entity for entity in sorted(entities, key=lambda item: item.id.value)
        if effective_kind(entity) == kind and entity.game_design is not None
    ]


def _consequence_entry(entity: SpecEntity | None, ref) -> dict:
    if entity is None or entity.game_design is None:
        return {"id": ref.value}
    design = entity.game_design
    return {
        "id": entity.id.value,
        "magnitude_source": design.magnitude_source,
        "handler": design.handler,
        "campaign_flags": list(design.campaign_flags),
    }


def _render_consequence(outcome: dict) -> str:
    parts = [outcome["id"]]
    if outcome.get("magnitude_source"):
        parts.append(f"(magnitude: {outcome['magnitude_source']})")
    if outcome.get("handler"):
        parts.append(f"[handler {outcome['handler']}]")
    if outcome.get("campaign_flags"):
        parts.append(f"[flags {', '.join(outcome['campaign_flags'])}]")
    return " ".join(parts)
