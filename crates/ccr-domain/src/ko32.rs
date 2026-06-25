// SPDX-License-Identifier: GPL-3.0-or-later
//! 32er modified Doppel-KO — frozen feeder graph (pure, no DB).
//!
//! The 32er has a DIFFERENT medal topology from 8er/16er (`_LB_STRUCTURE`):
//! partial double-elim with repechage feedback — the two repechage champions
//! fold back into a medal round (WB-SF winner × repechage champion), the final
//! draws from THOSE winners, and bronze = the medal-round LOSERS. So it is
//! graph-driven, not lambda-driven. Verbatim frozen copy of JF main.py
//! `_CANONICAL_32` (machine-checked vs ko_32.xls in edv/tests/test_ko_form_order.py;
//! feedback edges confirmed off the physical sheet, Decision 2026-06-02-2).
//!
//! Feeder: Los(n) seed | W(kf) winner of fight kf | L(kf) loser of fight kf.
//! Slot 1 = participant1 (top), slot 2 = participant2 (bottom).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feeder {
    Los(i32),
    W(i32),
    L(i32),
}

/// A bracket tree node: phase ("wb"|"lb"), round, pos.
pub type Node = (&'static str, i32, i32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Winner,
    Loser,
}

use Feeder::*;

/// `_CANONICAL_32[kf] = (top_feeder, bottom_feeder)` for kf 1..=61.
pub fn canonical_32() -> Vec<(i32, (Feeder, Feeder))> {
    vec![
        (1, (Los(1), Los(17))), (2, (Los(9), Los(25))),
        (3, (Los(5), Los(21))), (4, (Los(13), Los(29))),
        (5, (Los(3), Los(19))), (6, (Los(11), Los(27))),
        (7, (Los(7), Los(23))), (8, (Los(15), Los(31))),
        (9, (Los(2), Los(18))), (10, (Los(10), Los(26))),
        (11, (Los(6), Los(22))), (12, (Los(14), Los(30))),
        (13, (Los(4), Los(20))), (14, (Los(12), Los(28))),
        (15, (Los(8), Los(24))), (16, (Los(16), Los(32))),
        (17, (W(1), W(2))), (18, (W(3), W(4))),
        (19, (W(5), W(6))), (20, (W(7), W(8))),
        (21, (W(9), W(10))), (22, (W(11), W(12))),
        (23, (W(13), W(14))), (24, (W(15), W(16))),
        (25, (L(13), L(14))), (26, (L(15), L(16))),     // LB r0: R0 losers
        (27, (L(9), L(10))), (28, (L(11), L(12))),
        (29, (L(5), L(6))), (30, (L(7), L(8))),
        (31, (L(1), L(2))), (32, (L(3), L(4))),
        (33, (W(17), W(18))), (34, (W(19), W(20))),     // WB quarterfinals
        (35, (W(21), W(22))), (36, (W(23), W(24))),
        (37, (W(25), L(21))), (38, (W(26), L(22))),     // LB r1: R1 losers, cross
        (39, (W(27), L(23))), (40, (W(28), L(24))),
        (41, (W(29), L(17))), (42, (W(30), L(18))),
        (43, (W(31), L(19))), (44, (W(32), L(20))),
        (45, (W(33), W(34))), (46, (W(35), W(36))),     // WB semifinals
        (47, (W(37), W(38))), (48, (W(39), W(40))),     // LB r2: merge
        (49, (W(41), W(42))), (50, (W(43), W(44))),
        (51, (W(47), L(33))), (52, (W(48), L(34))),     // LB r3: QF losers, cross
        (53, (W(49), L(35))), (54, (W(50), L(36))),
        (55, (W(51), W(52))), (56, (W(53), W(54))),     // LB r4: merge
        (57, (W(55), L(46))), (58, (L(45), W(56))),     // LB semi: SF losers, cross
        (59, (W(45), W(57))), (60, (W(46), W(58))),     // medal round: SF winner × repechage champ
        (61, (W(59), W(60))),                            // final
    ]
}

const KF32_RANGES: &[(i32, i32, &str, i32)] = &[
    (1, 16, "wb", 0), (17, 24, "wb", 1), (33, 36, "wb", 2), (45, 46, "wb", 3),
    (25, 32, "lb", 0), (37, 44, "lb", 1), (47, 50, "lb", 2), (51, 54, "lb", 3),
    (55, 56, "lb", 4), (57, 58, "lb", 5), (59, 60, "lb", 6), (61, 61, "lb", 7),
];

/// Kampffolge-Nr → (phase, round, pos = kf - range_lo).
pub fn kf_to_node(kf: i32) -> Node {
    for &(lo, hi, phase, rnd) in KF32_RANGES {
        if lo <= kf && kf <= hi {
            return (phase, rnd, kf - lo);
        }
    }
    panic!("kf out of range: {kf}");
}

/// Forward consumers of a finished node: where its winner/loser flow next.
/// Returns [(dst_node, slot, kind)]. slot 1 = participant1, 2 = participant2.
pub fn consumers(node: Node) -> Vec<(Node, i32, Kind)> {
    let mut out = Vec::new();
    for (kf, (top, bottom)) in canonical_32() {
        let dst = kf_to_node(kf);
        for (slot, feeder) in [(1, top), (2, bottom)] {
            let (src_kf, kind) = match feeder {
                W(r) => (r, Kind::Winner),
                L(r) => (r, Kind::Loser),
                Los(_) => continue,
            };
            if kf_to_node(src_kf) == node {
                out.push((dst, slot, kind));
            }
        }
    }
    out
}

/// The final node (kf 61) and the two medal-round nodes (kf 59/60, whose LOSERS
/// take bronze). JF `_KO32_FINAL_NODE` / `_KO32_MEDAL_NODES`.
/// All distinct tree nodes (for eager materialization).
pub fn all_nodes() -> Vec<Node> {
    let mut set: std::collections::BTreeSet<Node> = std::collections::BTreeSet::new();
    for (kf, _) in canonical_32() {
        set.insert(kf_to_node(kf));
    }
    set.into_iter().collect()
}

pub fn final_node() -> Node {
    kf_to_node(61)
}
pub fn medal_nodes() -> (Node, Node) {
    (kf_to_node(59), kf_to_node(60))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Replay the graph with "lowest Los wins" — mirrors ko_form_decoder
    /// realised_order. Returns kf -> (los_a, los_b) and (1st,2nd,3rd,3rd).
    fn simulate() -> (HashMap<i32, (Option<i32>, Option<i32>)>, (i32, i32, i32, i32)) {
        let graph = canonical_32();
        let mut winner: HashMap<i32, i32> = HashMap::new();
        let mut loser: HashMap<i32, i32> = HashMap::new();
        let mut participants: HashMap<i32, (Option<i32>, Option<i32>)> = HashMap::new();
        let resolve = |f: Feeder, w: &HashMap<i32, i32>, l: &HashMap<i32, i32>| match f {
            Los(n) => Some(n),
            W(kf) => w.get(&kf).copied(),
            L(kf) => l.get(&kf).copied(),
        };
        for (kf, (top, bottom)) in graph {
            let a = resolve(top, &winner, &loser);
            let b = resolve(bottom, &winner, &loser);
            participants.insert(kf, (a, b));
            let present: Vec<i32> = [a, b].into_iter().flatten().collect();
            if !present.is_empty() {
                winner.insert(kf, *present.iter().min().unwrap());
                if present.len() == 2 {
                    loser.insert(kf, *present.iter().max().unwrap());
                }
            }
        }
        let placements = (winner[&61], loser[&61], loser[&59], loser[&60]);
        (participants, placements)
    }

    #[test]
    fn replay_matches_ko32_sheet_oracle() {
        // Oracle = edv/tests/ko_form_decoder.py realised_order(decode_form(32)).
        let (p, (first, second, third1, third2)) = simulate();
        assert_eq!((first, second, third1, third2), (1, 2, 4, 3), "placements");
        // Key fights incl. the feedback cross (kf57/58) and medal round (kf59/60).
        assert_eq!(p[&45], (Some(1), Some(3)));
        assert_eq!(p[&46], (Some(2), Some(4)));
        assert_eq!(p[&57], (Some(5), Some(4)));
        assert_eq!(p[&58], (Some(3), Some(6)));
        assert_eq!(p[&59], (Some(1), Some(4)));
        assert_eq!(p[&60], (Some(2), Some(3)));
        assert_eq!(p[&61], (Some(1), Some(2)));
        // LB ladder samples.
        assert_eq!(p[&25], (Some(20), Some(28)));
        assert_eq!(p[&37], (Some(20), Some(10)));
        assert_eq!(p[&47], (Some(10), Some(14)));
        assert_eq!(p[&51], (Some(10), Some(5)));
        assert_eq!(p[&55], (Some(5), Some(7)));
    }

    #[test]
    fn node_mapping_and_terminals() {
        assert_eq!(kf_to_node(1), ("wb", 0, 0));
        assert_eq!(kf_to_node(45), ("wb", 3, 0));
        assert_eq!(kf_to_node(25), ("lb", 0, 0));
        assert_eq!(kf_to_node(61), ("lb", 7, 0));
        assert_eq!(final_node(), ("lb", 7, 0));
        assert_eq!(medal_nodes(), (("lb", 6, 0), ("lb", 6, 1)));
        // kf45 (WB SF, wb r3 p0) feeds the medal round kf59 as winner, kf58 as loser.
        let c = consumers(("wb", 3, 0));
        assert!(c.contains(&(("lb", 6, 0), 1, Kind::Winner))); // W45 → kf59 top
        assert!(c.contains(&(("lb", 5, 1), 1, Kind::Loser))); // L45 → kf58 top
    }
}
