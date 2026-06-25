// SPDX-License-Identifier: GPL-3.0-or-later
//! Repechage ("KO mit doppelter Trostrunde") topology — pure (no DB).
//!
//! 32 (num_rounds=5) and 64 (=6) are OFFICIAL DJB forms with frozen structures
//! `_REPECHAGE_STRUCTURE[5]/[6]` (machine-checked vs repechage_{32,64}.xls). 8/16
//! (=3/4) have NO official form (those sizes are Doppel-KO) and are EXTRAPOLATED
//! ("ohne Gewähr", Merlin 2026-06-18); the 8er is structurally degenerate (pools
//! of 2, one victim each).
//!
//! Structure: the main draw 'wb' is a plain single-elim binary tree of N; its 4
//! quadrants are the pools A/B/C/D, pool winner = winner of wb(num_rounds-3, p).
//! Only the people the pool winner BEAT enter the repechage, by loss depth
//! ('plost', pool, level), level 1 = earliest loss (mini-KO R0). The 'rep' phase:
//! per pool a staircase (l1 vs l2, W vs l3, …), two half-merges (A/B, C/D), two
//! CROSSED bronze (rep-winner half × HF loser other half). Medals 1/2/3/3,
//! bronze = WINNER of the bronze fight, no 5th place.
//!
//! This generator reproduces the certified [5]/[6] structures (asserted in tests)
//! and applies the identical construction to 3/4.

use crate::ko32::{Kind, Node};

/// Dynamic repechage entry: (pool 0..3, level >=1) → which slot it fills.
pub type PlostKey = (u8, i32);

pub fn is_supported(num_rounds: u32) -> bool {
    (3..=6).contains(&num_rounds)
}

/// Mini-KO depth = number of rounds inside a pool = how many fighters the pool
/// winner beats = number of repechage levels. 32→3, 64→4, 16→2, 8→1.
pub fn levels(num_rounds: u32) -> i32 {
    num_rounds as i32 - 2
}

/// The wb round whose winners are the pool winners (= mini-KO final).
pub fn poolfinal_round(num_rounds: u32) -> i32 {
    num_rounds as i32 - 3
}

fn hf_round(num_rounds: u32) -> i32 {
    num_rounds as i32 - 2
}

/// rep round indices: (staircase_last, merge, bronze).
fn rep_rounds(num_rounds: u32) -> (i32, i32, i32) {
    let l = levels(num_rounds);
    if l <= 1 {
        // degenerate 8er: no staircase; rep r0 = direct merge, rep r1 = bronze.
        (-1, 0, 1)
    } else {
        let staircase_last = l - 2; // rep rounds 0..l-2
        (staircase_last, l - 1, l)
    }
}

pub fn final_node(num_rounds: u32) -> Node {
    ("wb", num_rounds as i32 - 1, 0)
}

pub fn bronze_nodes(num_rounds: u32) -> (Node, Node) {
    let (_, _, b) = rep_rounds(num_rounds);
    (("rep", b, 0), ("rep", b, 1))
}

/// All W/L edges of the graph (plost entries are dynamic, NOT here).
fn all_edges(num_rounds: u32) -> Vec<(Node, Node, i32, Kind)> {
    let nr = num_rounds as i32;
    let mut e = Vec::new();
    // WB binary tree: winner of wb(r,p) → wb(r+1, p/2).
    for r in 0..(nr - 1) {
        let nf = 1i32 << (nr - 1 - r);
        for p in 0..nf {
            let slot = if p % 2 == 0 { 1 } else { 2 };
            e.push((("wb", r, p), ("wb", r + 1, p / 2), slot, Kind::Winner));
        }
    }
    let (_stair_last, merge, bronze) = rep_rounds(num_rounds);
    let hf = hf_round(num_rounds);
    // HF losers cross into bronze (the only L edges).
    e.push((("wb", hf, 0), ("rep", bronze, 1), 2, Kind::Loser));
    e.push((("wb", hf, 1), ("rep", bronze, 0), 2, Kind::Loser));

    let l = levels(num_rounds);
    if l <= 1 {
        // degenerate: rep r0 = merge of single victims; bronze winners advance.
        e.push((("rep", 0, 0), ("rep", 1, 0), 1, Kind::Winner));
        e.push((("rep", 0, 1), ("rep", 1, 1), 1, Kind::Winner));
        return e;
    }
    // Staircase W edges: rep(k-1,p) winner → rep(k,p) slot1, k=1..=l-2.
    for k in 1..=(l - 2) {
        for p in 0..4 {
            e.push((("rep", k - 1, p), ("rep", k, p), 1, Kind::Winner));
        }
    }
    // Merge: rep(stair_last, p) winners pair A/B (→merge0) and C/D (→merge1).
    let sl = _stair_last;
    e.push((("rep", sl, 0), ("rep", merge, 0), 1, Kind::Winner));
    e.push((("rep", sl, 1), ("rep", merge, 0), 2, Kind::Winner));
    e.push((("rep", sl, 2), ("rep", merge, 1), 1, Kind::Winner));
    e.push((("rep", sl, 3), ("rep", merge, 1), 2, Kind::Winner));
    // Merge winners → bronze (slot1; HF losers fill slot2 via the L edges above).
    e.push((("rep", merge, 0), ("rep", bronze, 0), 1, Kind::Winner));
    e.push((("rep", merge, 1), ("rep", bronze, 1), 1, Kind::Winner));
    e
}

/// All distinct tree nodes (wb + rep) for eager materialization.
pub fn all_nodes(num_rounds: u32) -> Vec<Node> {
    let mut set: std::collections::BTreeSet<Node> = std::collections::BTreeSet::new();
    for (src, dst, _, _) in all_edges(num_rounds) {
        set.insert(src);
        set.insert(dst);
    }
    // rep r0 staircase / merge entries have no incoming W edge (plost-fed) — add them.
    for ((_pool, _level), (node, _slot)) in plost_targets(num_rounds) {
        set.insert(node);
    }
    set.insert(final_node(num_rounds));
    set.into_iter().collect()
}

/// Forward consumers of a node (W/L edges; plost is separate).
pub fn consumers(num_rounds: u32, node: Node) -> Vec<(Node, i32, Kind)> {
    all_edges(num_rounds)
        .into_iter()
        .filter(|(src, _, _, _)| *src == node)
        .map(|(_, dst, slot, kind)| (dst, slot, kind))
        .collect()
}

/// (pool, level) → (rep node, slot) for the dynamic plost entries.
pub fn plost_targets(num_rounds: u32) -> Vec<(PlostKey, (Node, i32))> {
    let l = levels(num_rounds);
    let mut out = Vec::new();
    if l <= 1 {
        // degenerate: pool victim (level 1) goes straight into the merge round 0.
        for p in 0..4u8 {
            let pos = (p / 2) as i32;
            let slot = if p % 2 == 0 { 1 } else { 2 };
            out.push(((p, 1), (("rep", 0, pos), slot)));
        }
        return out;
    }
    for p in 0..4u8 {
        let pos = p as i32;
        // level 1,2 fill rep r0 slot 1,2; level k>=3 fills rep r(k-2) slot 2.
        out.push(((p, 1), (("rep", 0, pos), 1)));
        out.push(((p, 2), (("rep", 0, pos), 2)));
        for k in 3..=l {
            out.push(((p, k), (("rep", k - 2, pos), 2)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    // Frozen certified structure translated to node space — the oracle for 32.
    // Source: JF _REPECHAGE_STRUCTURE[5] + _REPE_RANGES[5].
    fn consumer_set(num_rounds: u32) -> BTreeSet<(Node, Node, i32, &'static str)> {
        all_edges(num_rounds)
            .into_iter()
            .map(|(s, d, slot, k)| (s, d, slot, if k == Kind::Winner { "W" } else { "L" }))
            .collect()
    }

    #[test]
    fn rep32_matches_certified_structure() {
        // Spot-check the decisive edges of _REPECHAGE_STRUCTURE[5] (32 draw).
        let c = consumer_set(5);
        // Pool finals (wb r2) → HF (wb r3): binary.
        assert!(c.contains(&(("wb", 2, 0), ("wb", 3, 0), 1, "W")));
        assert!(c.contains(&(("wb", 2, 1), ("wb", 3, 0), 2, "W")));
        // HF (wb r3) → final (wb r4).
        assert!(c.contains(&(("wb", 3, 0), ("wb", 4, 0), 1, "W")));
        // Bronze CROSS: HF A/B loser (wb r3 p0) → bronze pos1; C/D loser → bronze pos0.
        assert!(c.contains(&(("wb", 3, 0), ("rep", 3, 1), 2, "L")));
        assert!(c.contains(&(("wb", 3, 1), ("rep", 3, 0), 2, "L")));
        // Staircase: rep r0 winner → rep r1 slot1; rep r1 → merge; merge → bronze.
        assert!(c.contains(&(("rep", 0, 0), ("rep", 1, 0), 1, "W")));
        assert!(c.contains(&(("rep", 1, 0), ("rep", 2, 0), 1, "W")));
        assert!(c.contains(&(("rep", 1, 1), ("rep", 2, 0), 2, "W")));
        assert!(c.contains(&(("rep", 2, 0), ("rep", 3, 0), 1, "W")));
        // plost: A1→rep r0 p0 s1, A2→s2, A3→rep r1 p0 s2.
        let pt: std::collections::HashMap<_, _> = plost_targets(5).into_iter().collect();
        assert_eq!(pt[&(0, 1)], (("rep", 0, 0), 1));
        assert_eq!(pt[&(0, 2)], (("rep", 0, 0), 2));
        assert_eq!(pt[&(0, 3)], (("rep", 1, 0), 2));
        assert_eq!(final_node(5), ("wb", 4, 0));
        assert_eq!(bronze_nodes(5), (("rep", 3, 0), ("rep", 3, 1)));
        assert_eq!(poolfinal_round(5), 2);
        assert_eq!(levels(5), 3);
    }

    #[test]
    fn rep64_structure() {
        assert_eq!(levels(6), 4);
        assert_eq!(poolfinal_round(6), 3);
        assert_eq!(final_node(6), ("wb", 5, 0));
        assert_eq!(bronze_nodes(6), (("rep", 4, 0), ("rep", 4, 1)));
        let c = consumer_set(6);
        // 64er bronze cross: HF (wb r4) losers → rep bronze (r4).
        assert!(c.contains(&(("wb", 4, 0), ("rep", 4, 1), 2, "L")));
        assert!(c.contains(&(("wb", 4, 1), ("rep", 4, 0), 2, "L")));
        // staircase has 3 rounds (r0,r1,r2); level 4 enters at rep r2.
        let pt: std::collections::HashMap<_, _> = plost_targets(6).into_iter().collect();
        assert_eq!(pt[&(0, 4)], (("rep", 2, 0), 2));
    }

    #[test]
    fn rep16_extrapolated_is_coherent() {
        // 16er: 2 levels, staircase = just rep r0 (l1 vs l2), merge r1, bronze r2.
        assert_eq!(levels(4), 2);
        assert_eq!(bronze_nodes(4), (("rep", 2, 0), ("rep", 2, 1)));
        let pt: std::collections::HashMap<_, _> = plost_targets(4).into_iter().collect();
        assert_eq!(pt[&(0, 1)], (("rep", 0, 0), 1));
        assert_eq!(pt[&(0, 2)], (("rep", 0, 0), 2));
        let c = consumer_set(4);
        assert!(c.contains(&(("rep", 0, 0), ("rep", 1, 0), 1, "W"))); // merge
        assert!(c.contains(&(("rep", 1, 0), ("rep", 2, 0), 1, "W"))); // bronze
    }
}
