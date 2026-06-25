// SPDX-License-Identifier: GPL-3.0-or-later
//! Pool placement tiebreaker — pure (no DB).
//!
//! DJB hierarchy (Decision 2026-06-08, CLAUDE.md; mirror of JF main.py
//! `_compute_pool_standings` + edv `tournament_service.compute_pool_standings`):
//!   1. total wins (winner over finished/bye fights)
//!   2. tie → direct comparison = wins ONLY vs the other tied members
//!      (2-way = duel winner; 3+-way = count of beaten tie members)
//!   3. true cycle → stable gp_id (deterministic Los)
//! POINTS / plus-minus do NOT count.
//!
//! Oracle: the cases in tests below are ported verbatim from
//! JudgeFrontend/tests/test_pool_tiebreaker.py (and edv's twin).

use std::collections::{BTreeSet, HashMap};

/// One pool fight as the tiebreaker needs it. `decided` = status in
/// (finished, bye). A bye is encoded p1 == p2 with winner = p1 (counts as a win
/// but not as head-to-head). Mirrors JF main.py:920-937.
#[derive(Debug, Clone)]
pub struct PoolFight {
    pub p1: Option<i32>,
    pub p2: Option<i32>,
    pub winner: Option<i32>,
    pub decided: bool,
}

/// Return the ordered gp_ids [1st, 2nd, 3rd, ...] for a pool.
pub fn pool_standings(fights: &[PoolFight]) -> Vec<i32> {
    // Participant set from all fights (JF lines 908-912).
    let mut gp_set: BTreeSet<i32> = BTreeSet::new();
    for f in fights {
        if let Some(p1) = f.p1 {
            gp_set.insert(p1);
        }
        if let Some(p2) = f.p2 {
            if Some(p2) != f.p1 {
                gp_set.insert(p2);
            }
        }
    }

    let mut wins: HashMap<i32, i32> = gp_set.iter().map(|&g| (g, 0)).collect();
    // head_to_head: sorted (a,b) -> winner gp. JF line 936.
    let mut h2h: HashMap<(i32, i32), i32> = HashMap::new();

    for f in fights {
        if !f.decided {
            continue;
        }
        let (Some(p1), Some(p2)) = (f.p1, f.p2) else {
            continue;
        };
        if let Some(w) = f.winner {
            *wins.entry(w).or_insert(0) += 1;
            if p1 != p2 {
                let key = if p1 <= p2 { (p1, p2) } else { (p2, p1) };
                h2h.insert(key, w);
            }
        }
    }

    let win = |g: i32| *wins.get(&g).unwrap_or(&0);

    // Base order: wins desc, gp_id asc (BTreeSet already gives gp_id asc).
    let mut base: Vec<i32> = gp_set.iter().copied().collect();
    base.sort_by_key(|&g| (-win(g), g));

    // Within each equal-wins run (>1), re-sort by subgroup head-to-head wins.
    let subgroup_h2h = |gp: i32, run: &[i32]| -> i32 {
        run.iter()
            .filter(|&&other| {
                other != gp && {
                    let key = if gp <= other { (gp, other) } else { (other, gp) };
                    h2h.get(&key) == Some(&gp)
                }
            })
            .count() as i32
    };

    let mut ordered: Vec<i32> = Vec::with_capacity(base.len());
    let mut i = 0;
    while i < base.len() {
        let mut j = i;
        while j < base.len() && win(base[j]) == win(base[i]) {
            j += 1;
        }
        let mut run = base[i..j].to_vec();
        if run.len() > 1 {
            let counts: HashMap<i32, i32> =
                run.iter().map(|&g| (g, subgroup_h2h(g, &run))).collect();
            run.sort_by_key(|&g| (-counts[&g], g));
        }
        ordered.extend(run);
        i = j;
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(p1: i32, p2: i32, winner: i32) -> PoolFight {
        PoolFight { p1: Some(p1), p2: Some(p2), winner: Some(winner), decided: true }
    }

    #[test]
    fn h2h_beats_points() {
        // 4er pool: 1 and 2 each 2 wins; 2 beats 1 directly; 1 has huge points.
        // JF test_h2h_beats_points → [2, 1, {3,4}].
        let fights = [f(1, 3, 1), f(1, 4, 1), f(2, 1, 2), f(2, 3, 2), f(3, 4, 3), f(4, 2, 4)];
        let order = pool_standings(&fights);
        assert_eq!(order[0], 2, "{order:?}");
        assert_eq!(order[1], 1, "{order:?}");
        let tail: BTreeSet<i32> = order[2..].iter().copied().collect();
        assert_eq!(tail, BTreeSet::from([3, 4]));
    }

    #[test]
    fn three_way_circle_falls_to_id() {
        // 1→2, 2→3, 3→1, all 1 win → stable gp_id. JF test_three_way_circle.
        let fights = [f(1, 2, 1), f(2, 3, 2), f(1, 3, 3)];
        assert_eq!(pool_standings(&fights), vec![1, 2, 3]);
    }

    #[test]
    fn transitive_pool_unaffected() {
        // 1>2>3 transitive. JF test_transitive_pool_unaffected.
        let fights = [f(1, 2, 1), f(1, 3, 1), f(2, 3, 2)];
        assert_eq!(pool_standings(&fights), vec![1, 2, 3]);
    }
}
