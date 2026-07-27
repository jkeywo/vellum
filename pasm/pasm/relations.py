"""Shared traversal over declared PASM entity relationships.

Closure here is over the *declared* model only. Observed source-file edges stay
in `implementation.observation`; PASM does not claim that a declared path
implies a runtime one, or the reverse.
"""

from __future__ import annotations

from collections import deque

from pasm.core.model import EntityId, SpecEntity

DEPENDENCY_FIELDS = (
    "depends_on",
    "runtime_depends_on",
    "build_depends_on",
    "optional_dependency",
    "temporary_dependency",
)


def relation_targets(architecture, fields: tuple[str, ...]) -> tuple[EntityId, ...]:
    """Collect relationship targets from an architecture section, in field order."""
    if architecture is None:
        return ()
    targets: list[EntityId] = []
    for name in fields:
        value = getattr(architecture, name, None)
        if value is None:
            continue
        if isinstance(value, tuple):
            targets.extend(value)
        else:
            targets.append(value)
    return tuple(targets)


def build_graph(
    entities: tuple[SpecEntity, ...],
    fields: tuple[str, ...] = DEPENDENCY_FIELDS,
) -> dict[EntityId, tuple[EntityId, ...]]:
    """Adjacency over resolvable declared edges.

    Edges to unknown entities are dropped: unresolved references already produce
    their own finding, and keeping them here would make every closure result
    depend on a separate error.
    """
    known = {entity.id for entity in entities}
    graph: dict[EntityId, tuple[EntityId, ...]] = {}
    for entity in entities:
        seen: list[EntityId] = []
        for target in relation_targets(entity.architecture, fields):
            if target in known and target not in seen:
                seen.append(target)
        graph[entity.id] = tuple(seen)
    return graph


def shortest_path(
    graph: dict[EntityId, tuple[EntityId, ...]],
    start: EntityId,
    goal: EntityId,
) -> tuple[EntityId, ...]:
    """Shortest declared path from start to goal, or () when unreachable.

    A start equal to goal returns the shortest cycle back to itself rather than
    an empty walk.
    """
    previous: dict[EntityId, EntityId] = {}
    queue = deque([start])
    visited = {start}
    found = False
    while queue and not found:
        current = queue.popleft()
        for target in graph.get(current, ()):
            if target == goal:
                previous[target] = current
                found = True
                break
            if target not in visited:
                visited.add(target)
                previous[target] = current
                queue.append(target)
    if not found:
        return ()
    path = [goal]
    while path[-1] != start or len(path) == 1:
        path.append(previous[path[-1]])
    return tuple(reversed(path))


def reachable(graph: dict[EntityId, tuple[EntityId, ...]], start: EntityId) -> set[EntityId]:
    """Every entity reachable from start, excluding start unless it is in a cycle."""
    found: set[EntityId] = set()
    queue = deque(graph.get(start, ()))
    while queue:
        current = queue.popleft()
        if current in found:
            continue
        found.add(current)
        queue.extend(graph.get(current, ()))
    return found


def cyclic_components(
    graph: dict[EntityId, tuple[EntityId, ...]],
) -> tuple[tuple[EntityId, ...], ...]:
    """Groups of mutually reachable entities, plus self-dependencies.

    Reported per component rather than per cycle: a component with four members
    can hold many overlapping cycles, and one finding naming the members is more
    actionable than an enumeration of them.
    """
    closures = {node: reachable(graph, node) for node in graph}
    cyclic = sorted(node for node in graph if node in closures[node])
    components: list[tuple[EntityId, ...]] = []
    assigned: set[EntityId] = set()
    for node in cyclic:
        if node in assigned:
            continue
        members = tuple(
            sorted(
                other
                for other in cyclic
                if other == node or (other in closures[node] and node in closures[other])
            )
        )
        assigned.update(members)
        components.append(members)
    return tuple(components)
