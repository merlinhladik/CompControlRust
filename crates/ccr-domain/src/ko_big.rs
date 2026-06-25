// SPDX-License-Identifier: GPL-3.0-or-later
//! Large modified Doppel-KO via a GENERATED feeder graph (facade over sizes).
//!
//! ⚠ EXTRAPOLATION — NOT DJB-sheet-certified. There is no official 64er
//! Doppel-KO bracket (64 = repechage in the DJB system), so no `ko_64.xls` and
//! no decoder oracle exists. This module generalizes the 32er's STRUCTURAL
//! pattern one level deeper with a single uniform rule:
//!   WB rounds 0..SF (binary) → LB ladder alternating
//!     [cross-with-WB-round-k-losers] / [binary merge] → feedback medal round
//!     (WB-SF winner × LB champion) → final.
//!   Crosses use a uniform half-swap (pos XOR nf/2). Merges are binary.
//!   R0 losers pair quadrant-reversed.
//! Medal placements are correct ONLY by this construction rule — they are NOT
//! verified against any official sheet (Merlin authorized "ohne Gewähr",
//! 2026-06-18). Rollback = delete this module + the num_rounds==6 dispatch.
//!
//! The certified 32er (`ko32`, decoded from ko_32.xls) is NOT replaced: this
//! generator differs from it at the two DJB-specific irregularities (lb r3 uses
//! identity-not-cross, lb r5 has a slot-swap). So `consumers`/`final_node`/
//! `medal_nodes` here DELEGATE to `ko32` for num_rounds==5 and only generate for
//! larger sizes.

use crate::ko32::{Kind, Node};

#[derive(Clone, Copy, Debug)]
enum Gen {
    Seed(#[allow(dead_code)] i32), // read only in the replay test
    Win(Node),
    Lose(Node),
}

/// Standard balanced bracket seeding (length 2^num_rounds): 1,N,…opposite-side.
fn standard_seed(num_rounds: u32) -> Vec<i32> {
    let mut res = vec![1, 2];
    for _ in 0..(num_rounds - 1) {
        let m = res.len() * 2 + 1;
        let mut next = Vec::with_capacity(res.len() * 2);
        for x in res {
            next.push(x);
            next.push(m as i32 - x);
        }
        res = next;
    }
    res
}

/// R0-loser pair order: groups of 2 pairs, group order reversed (matches 32er).
fn quadrant_reverse(num_pairs: usize) -> Vec<usize> {
    let groups = num_pairs / 2;
    (0..num_pairs)
        .map(|p| (groups - 1 - p / 2) * 2 + (p % 2))
        .collect()
}

/// Build the generated feeder graph for `num_rounds` (>=5). Returns
/// (nodes_with_feeders, final_node, (medal0, medal1)).
fn build(num_rounds: u32) -> (Vec<(Node, (Gen, Gen))>, Node, (Node, Node)) {
    let nr = num_rounds;
    let big: usize = 1 << nr;
    let wb_last = (nr - 2) as i32; // SF round (2 fights); no WB final
    let mut g: Vec<(Node, (Gen, Gen))> = Vec::new();

    // WB tree.
    let seeds = standard_seed(nr);
    for r in 0..=wb_last {
        let nf = 1usize << (nr - 1 - r as u32);
        for p in 0..nf {
            let node: Node = ("wb", r, p as i32);
            let feeders = if r == 0 {
                (Gen::Seed(seeds[2 * p]), Gen::Seed(seeds[2 * p + 1]))
            } else {
                (Gen::Win(("wb", r - 1, 2 * p as i32)), Gen::Win(("wb", r - 1, 2 * p as i32 + 1)))
            };
            g.push((node, feeders));
        }
    }

    // LB r0: R0 losers paired, quadrant-reversed.
    let lb0 = big / 4;
    let order = quadrant_reverse(lb0);
    for (p, &pair) in order.iter().enumerate() {
        g.push((
            ("lb", 0, p as i32),
            (Gen::Lose(("wb", 0, (2 * pair) as i32)), Gen::Lose(("wb", 0, (2 * pair + 1) as i32))),
        ));
    }

    // Alternating cross (consume WB round k losers) / merge.
    let mut cur = 0i32;
    for k in 1..=wb_last {
        let cross = cur + 1;
        let nf = 1usize << (nr - 1 - k as u32); // WB round k fight count = losers
        let half = (nf / 2) as i32;
        for p in 0..nf {
            g.push((
                ("lb", cross, p as i32),
                (Gen::Win(("lb", cur, p as i32)), Gen::Lose(("wb", k, p as i32 ^ half))),
            ));
        }
        cur = cross;
        if k < wb_last {
            let merge = cur + 1;
            for p in 0..(nf / 2) {
                g.push((
                    ("lb", merge, p as i32),
                    (Gen::Win(("lb", cur, 2 * p as i32)), Gen::Win(("lb", cur, 2 * p as i32 + 1))),
                ));
            }
            cur = merge;
        }
    }

    // Medal round (feedback: WB-SF winner × LB champion) + final.
    let medal = cur + 1;
    for p in 0..2i32 {
        g.push((
            ("lb", medal, p),
            (Gen::Win(("wb", wb_last, p)), Gen::Win(("lb", cur, p))),
        ));
    }
    let final_r = medal + 1;
    g.push((("lb", final_r, 0), (Gen::Win(("lb", medal, 0)), Gen::Win(("lb", medal, 1)))));

    (g, ("lb", final_r, 0), (("lb", medal, 0), ("lb", medal, 1)))
}

/// Forward consumers of a node: where its winner/loser flow. Delegates to the
/// certified `ko32` for the 32 draw; generates for larger sizes.
pub fn consumers(num_rounds: u32, node: Node) -> Vec<(Node, i32, Kind)> {
    if num_rounds == 5 {
        return crate::ko32::consumers(node);
    }
    let (graph, _, _) = build(num_rounds);
    let mut out = Vec::new();
    for (dst, (top, bottom)) in graph {
        for (slot, feeder) in [(1, top), (2, bottom)] {
            let (src, kind) = match feeder {
                Gen::Win(s) => (s, Kind::Winner),
                Gen::Lose(s) => (s, Kind::Loser),
                Gen::Seed(_) => continue,
            };
            if src == node {
                out.push((dst, slot, kind));
            }
        }
    }
    out
}

pub fn final_node(num_rounds: u32) -> Node {
    if num_rounds == 5 {
        return crate::ko32::final_node();
    }
    build(num_rounds).1
}

pub fn medal_nodes(num_rounds: u32) -> (Node, Node) {
    if num_rounds == 5 {
        return crate::ko32::medal_nodes();
    }
    build(num_rounds).2
}

/// Sizes this engine drives. 5 = certified 32er; 6 = extrapolated 64er.
pub fn is_supported(num_rounds: u32) -> bool {
    num_rounds == 5 || num_rounds == 6
}

/// All distinct tree nodes (for eager materialization).
pub fn all_nodes(num_rounds: u32) -> Vec<Node> {
    if num_rounds == 5 {
        return crate::ko32::all_nodes();
    }
    let (graph, _, _) = build(num_rounds);
    let mut set: std::collections::BTreeSet<Node> = std::collections::BTreeSet::new();
    for (node, _) in graph {
        set.insert(node);
    }
    set.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Replay "lowest los wins" over the generated graph; assert it is a fully
    /// connected, internally-consistent bracket (every fight resolves to two
    /// fighters, 4 distinct medals). This is the ONLY available check — there is
    /// no DJB oracle for 64.
    #[test]
    fn gen64_is_structurally_complete() {
        let nr = 6;
        let (graph, final_node, (m0, m1)) = build(nr);
        // dependency order: all wb rounds, then lb rounds ascending.
        let mut order = graph.clone();
        let rank = |n: &Node| (if n.0 == "wb" { 0 } else { 1 }, n.1);
        order.sort_by_key(|(n, _)| rank(n));

        let mut winner: HashMap<Node, i32> = HashMap::new();
        let mut loser: HashMap<Node, i32> = HashMap::new();
        let resolve = |f: &Gen, w: &HashMap<Node, i32>, l: &HashMap<Node, i32>| match f {
            Gen::Seed(n) => Some(*n),
            Gen::Win(s) => w.get(s).copied(),
            Gen::Lose(s) => l.get(s).copied(),
        };
        let mut unresolved = 0;
        for (node, (top, bottom)) in &order {
            let a = resolve(top, &winner, &loser);
            let b = resolve(bottom, &winner, &loser);
            if a.is_none() || b.is_none() {
                unresolved += 1;
                continue;
            }
            let (a, b) = (a.unwrap(), b.unwrap());
            winner.insert(*node, a.min(b));
            loser.insert(*node, a.max(b));
        }
        assert_eq!(unresolved, 0, "every fight must resolve to two fighters");
        assert_eq!(graph.len(), 125, "64er has 125 fights (62 WB + 63 LB)");

        let first = winner[&final_node];
        let second = loser[&final_node];
        let third1 = loser[&m0];
        let third2 = loser[&m1];
        let medals = [first, second, third1, third2];
        let distinct: std::collections::BTreeSet<i32> = medals.iter().copied().collect();
        assert_eq!(distinct.len(), 4, "four distinct medallists: {medals:?}");
        assert_eq!(first, 1, "lowest los wins everything in WB");
    }

    #[test]
    fn round_structure_64() {
        let (_, final_node, (m0, _)) = build(6);
        assert_eq!(final_node, ("lb", 9, 0)); // r0..r9
        assert_eq!(m0, ("lb", 8, 0)); // medal round
        // WB rounds 0..4 (SF), counts 32,16,8,4,2.
        let g = build(6).0;
        let count = |ph: &str, r: i32| g.iter().filter(|(n, _)| n.0 == ph && n.1 == r).count();
        assert_eq!([count("wb", 0), count("wb", 1), count("wb", 2), count("wb", 3), count("wb", 4)],
                   [32, 16, 8, 4, 2]);
        assert_eq!([count("lb", 0), count("lb", 1), count("lb", 2), count("lb", 3)], [16, 16, 8, 8]);
    }
}
