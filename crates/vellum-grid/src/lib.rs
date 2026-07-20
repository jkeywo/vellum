//! Deterministic shortest-path search over a grid, priced by the caller.
//!
//! # Why this takes indices instead of positions
//!
//! Both games serialise their coordinate and direction types into saved state:
//! one game's `Direction` rides inside recorded commands as a postcard variant
//! index, the other's `Pos` sits in a world whose RON text *is* the mission
//! fingerprint. Moving those types into a shared crate would rewrite both save
//! formats for no gain, so they stay where they are and this works in terms
//! the caller supplies: a node is a `usize` into the caller's own grid, a move
//! is a `u8` index into the caller's own direction table.
//!
//! That sounds austere and buys something real. This crate has no opinion
//! about storeys, stairs, map bounds, doors, or what "adjacent" means, so a
//! game with multiple floors and one with a single 32×20 map use the same
//! search without either bending to the other.
//!
//! # What it replaces
//!
//! Three hand-rolled searches across two games, all the same algorithm. The
//! third was written during this extraction, because the first two could not
//! be reused: one of the games needed a route that avoided a guard's line of
//! sight, its existing pathfinder had no cost hook, and copying eighty lines
//! of Dijkstra was faster than threading one through. That is the shape of
//! duplication this exists to stop.
//!
//! # Determinism
//!
//! Ties break on insertion order, never on address or hash iteration. Equal-
//! cost routes are therefore resolved by the order the caller yields
//! neighbours in, which makes the chosen route a pure function of the caller's
//! own data — the property both games' replays depend on.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// One reachable neighbour: where it is, which move reaches it, what it costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// The caller's index for the node this step lands on.
    ///
    /// It need not be the node "next to" the one being expanded. A game with
    /// stairs can land a step somewhere else entirely, which is how a
    /// multi-storey map fits a flat search without this crate knowing about
    /// storeys.
    pub node: usize,
    /// Which of the caller's moves this was — an index into whatever direction
    /// table the caller keeps.
    pub move_index: u8,
    /// What taking it costs. Callers price terrain, trespass, or being seen
    /// here; the search only adds.
    pub cost: u32,
}

/// The first move of a cheapest route from `start` to `goal`.
///
/// Returns `None` when the goal is unreachable, or when `start == goal` and
/// there is therefore no move to make. Only the first move is returned because
/// that is all either game ever wanted: both re-plan every turn, since the
/// world moves underneath a stored route.
///
/// `neighbours` is called once per expanded node and should push every legal
/// step from it. Pushing in a fixed order is what makes ties deterministic.
pub fn first_move_towards(
    size: usize,
    start: usize,
    goal: usize,
    mut neighbours: impl FnMut(usize, &mut Vec<Step>),
) -> Option<u8> {
    if start == goal || start >= size || goal >= size {
        return None;
    }

    let mut best = vec![u32::MAX; size];
    let mut first_move: Vec<Option<u8>> = vec![None; size];
    best[start] = 0;

    // Cost first, then insertion order: equal-cost nodes expand in the order
    // they were found, so the route never depends on how the heap happens to
    // arrange equal keys.
    let mut heap: BinaryHeap<Reverse<(u32, u32, usize)>> = BinaryHeap::new();
    let mut sequence: u32 = 0;
    heap.push(Reverse((0, sequence, start)));

    let mut scratch = Vec::new();
    while let Some(Reverse((cost, _, node))) = heap.pop() {
        if cost > best[node] {
            continue; // a stale entry left behind by a cheaper route
        }
        if node == goal {
            return first_move[node];
        }

        scratch.clear();
        neighbours(node, &mut scratch);
        for step in &scratch {
            if step.node >= size {
                continue;
            }
            let next = cost.saturating_add(step.cost);
            if next >= best[step.node] {
                continue;
            }
            best[step.node] = next;
            first_move[step.node] = if node == start {
                Some(step.move_index)
            } else {
                first_move[node]
            };
            sequence += 1;
            heap.push(Reverse((next, sequence, step.node)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 5x5 open grid, four-way, uniform cost. Moves are indexed
    /// 0=north 1=east 2=south 3=west, pushed in that order.
    fn open_grid(width: usize, height: usize, blocked: &[usize]) -> impl Fn(usize, &mut Vec<Step>) {
        let blocked = blocked.to_vec();
        move |node, out: &mut Vec<Step>| {
            let (x, y) = (node % width, node / width);
            let candidates = [
                (0u8, x as isize, y as isize - 1),
                (1, x as isize + 1, y as isize),
                (2, x as isize, y as isize + 1),
                (3, x as isize - 1, y as isize),
            ];
            for (move_index, nx, ny) in candidates {
                if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                    continue;
                }
                let node = ny as usize * width + nx as usize;
                if blocked.contains(&node) {
                    continue;
                }
                out.push(Step {
                    node,
                    move_index,
                    cost: 1,
                });
            }
        }
    }

    #[test]
    fn a_straight_line_takes_the_straight_step() {
        // (0,0) to (3,0): east.
        let step = first_move_towards(25, 0, 3, open_grid(5, 5, &[])).expect("reachable");
        assert_eq!(step, 1);
    }

    #[test]
    fn an_unreachable_goal_has_no_move() {
        // Wall the goal in completely.
        let walls = [3, 9, 13, 7];
        assert_eq!(
            first_move_towards(25, 0, 8, open_grid(5, 5, &walls)),
            None,
            "a sealed goal should be unreachable"
        );
    }

    #[test]
    fn standing_on_the_goal_is_not_a_move() {
        assert_eq!(first_move_towards(25, 6, 6, open_grid(5, 5, &[])), None);
    }

    #[test]
    fn the_route_goes_around_a_wall() {
        // Block the direct easterly run at (1,0); the route must still arrive.
        let step = first_move_towards(25, 0, 2, open_grid(5, 5, &[1])).expect("reachable");
        assert_ne!(step, 1, "it stepped into the wall");
    }

    /// The property both games' replays rest on: same input, same answer,
    /// every time.
    #[test]
    fn the_same_grid_always_yields_the_same_move() {
        for _ in 0..32 {
            let step = first_move_towards(25, 12, 0, open_grid(5, 5, &[7]));
            assert_eq!(step, first_move_towards(25, 12, 0, open_grid(5, 5, &[7])));
            assert!(step.is_some());
        }
    }

    /// Cost is the whole point of the hook: an expensive tile should be walked
    /// around, and a cheap one through.
    #[test]
    fn price_changes_the_route() {
        let width = 5;
        // Two routes east from (0,1) to (2,1): straight through (1,1), or
        // around via (1,0). Make the straight one expensive.
        let expensive = |node: usize, out: &mut Vec<Step>| {
            let (x, y) = (node % width, node / width);
            let candidates = [
                (0u8, x as isize, y as isize - 1),
                (1, x as isize + 1, y as isize),
                (2, x as isize, y as isize + 1),
                (3, x as isize - 1, y as isize),
            ];
            for (move_index, nx, ny) in candidates {
                if nx < 0 || ny < 0 || nx >= 5 || ny >= 5 {
                    continue;
                }
                let node = ny as usize * width + nx as usize;
                out.push(Step {
                    node,
                    move_index,
                    // (1,1) is node 6 and costs a detour's worth to enter.
                    cost: if node == 6 { 50 } else { 1 },
                });
            }
        };
        let step = first_move_towards(25, 5, 7, expensive).expect("reachable");
        assert_eq!(step, 0, "it should have gone north around the costly tile");
    }

    /// A step may land somewhere non-adjacent, which is how stairs work in a
    /// game with storeys.
    #[test]
    fn a_step_may_teleport() {
        let jump = |node: usize, out: &mut Vec<Step>| {
            if node == 0 {
                out.push(Step {
                    node: 24,
                    move_index: 7,
                    cost: 1,
                });
            }
        };
        assert_eq!(first_move_towards(25, 0, 24, jump), Some(7));
    }

    #[test]
    fn out_of_range_nodes_are_refused_rather_than_panicking() {
        assert_eq!(first_move_towards(4, 9, 1, |_, _| {}), None);
        assert_eq!(first_move_towards(4, 1, 9, |_, _| {}), None);
    }
}
