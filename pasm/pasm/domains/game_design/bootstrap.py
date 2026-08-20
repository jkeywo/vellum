"""Draft skeleton design entities from an authored world file.

The authored content is authoritative; bootstrap turns it into a spec
skeleton an LLM fills with intent and a human ratifies through `pasm
review`. Every drafted entity carries `origin: ai` and draft status, so the
skeleton validates with warnings (missing intent to fill in), never silent
completeness.
"""

from __future__ import annotations

from pathlib import Path

from pasm.scanners.content_toml import ContentParseError, extract_content_document


DEFAULT_DEADLINE_TABLE = "deadline"
DEFAULT_DEADLINE_ID_KEY = "id"


class BootstrapError(Exception):
    pass


def bootstrap_scenario(
    world_file: str,
    workspace_root: Path,
    scenario_id: str | None = None,
    deadline_table: str = DEFAULT_DEADLINE_TABLE,
    deadline_id_key: str = DEFAULT_DEADLINE_ID_KEY,
) -> str:
    world_path = (workspace_root / world_file).resolve()
    if not world_path.is_file():
        raise BootstrapError(f"World file '{world_file}' does not exist under the workspace root.")
    try:
        document = extract_content_document(world_path.read_text(encoding="utf-8"))
    except (OSError, ContentParseError) as exc:
        raise BootstrapError(f"World file '{world_file}' could not be parsed: {exc}") from exc

    model_id = scenario_id or _slug(world_path.stem)
    world_posix = Path(world_file).as_posix()

    lines: list[str] = [
        f"# Drafted by `pasm design bootstrap` from {world_posix}.",
        "# [ai] Every entity below is a skeleton: fill in causal intent from the",
        "# world file's design commentary and the GDD, then ratify via `pasm review`.",
        "entities:",
        f"  - scenario_model: {model_id}",
        "    core:",
        "      status: proposed",
        "      confidence: provisional",
        "      origin: ai",
        f"      title: {_title(model_id)} Scenario Model",
        "      summary: >",
        f"        [ai] Closed-world design model for {world_posix}; drafted from the",
        "        authored content, awaiting design intent.",
        "    game_design:",
        f"      world_file: {world_posix}",
    ]
    if deadline_table != DEFAULT_DEADLINE_TABLE:
        lines.append(f"      deadline_table: {deadline_table}")
    if deadline_id_key != DEFAULT_DEADLINE_ID_KEY:
        lines.append(f"      deadline_id_key: {deadline_id_key}")

    for row in document.rows(deadline_table):
        deadline_id = row.values.get(deadline_id_key)
        if not isinstance(deadline_id, str):
            continue
        gate_id = f"gate-{_slug(deadline_id)}"
        line_note = f" (authored at line {row.line})" if row.line is not None else ""
        lines.extend([
            f"  - gate: {gate_id}",
            "    core:",
            "      status: proposed",
            "      confidence: provisional",
            "      origin: ai",
            f"      title: {_title(deadline_id)} Deadline",
            "      summary: >",
            f"        [ai] Drafted from authored deadline '{deadline_id}'{line_note}.",
            "        State what failing this deadline causes, or mark it benign.",
            "      open_questions:",
            # PASM strings containing ": " must be quoted or the restricted
            # parser reads them as mappings.
            f"        - \"What does failing '{deadline_id}' cause? Add 'on_failure' or 'benign: true'.\"",
            "        - \"Which player verbs drive this gate? Add 'requires_player_action' or 'self_resolving: true'.\"",
            "    game_design:",
            f"      deadline_id: {deadline_id}",
        ])

    handlers = sorted({name for name, _ in document.script_functions})
    if handlers:
        lines.append("# [ai] Script handlers observed in the world file; wire consequences to them")
        lines.append("# via 'game_design.handler' as the causal intent is filled in:")
        for name in handlers:
            lines.append(f"#   - {name}")

    return "\n".join(lines) + "\n"


def _slug(value: str) -> str:
    slug = "".join(char if char.isalnum() else "-" for char in value.lower())
    while "--" in slug:
        slug = slug.replace("--", "-")
    return slug.strip("-")


def _title(value: str) -> str:
    return " ".join(part.capitalize() for part in _slug(value).split("-"))
