// SPDX-License-Identifier: GPL-3.0-or-later
//! Bracket-generation core — pure (no DB).
//!   - `recommend_bracket_type`: pick the bracket type from the fighter count
//!     (CLAUDE.md GenerationMethods defaults).
//!   - `pool_fight_schedule`: the single source of pool fight order for the whole
//!     suite (mirror of edv `pool_renderer._generate_fight_schedule`).
//!
//! Mirrors edv `data_transformation_pipeline._recommend_generation_method` +
//! `pool_renderer._generate_fight_schedule`.

/// Bracket type for `n` fighters. U9/U11 always pool (configurable pool sizes).
/// Thresholds (WSP/CLAUDE.md): <3 special · 3–5 pools · 6–10 double ·
/// 11–32 ko · 33–64 repechage · >64 deferred (capped to special + warn).
pub fn recommend_bracket_type(n: usize, is_u9_or_u11: bool) -> &'static str {
    if is_u9_or_u11 {
        return "pools";
    }
    match n {
        0..=2 => "special",
        3..=5 => "pools",
        6..=10 => "double",
        11..=32 => "ko",
        33..=64 => "repechage",
        _ => "special", // >64 deferred
    }
}

/// Split weight-sorted youth fighters into pools, returning index groups into
/// the input. Two-stage, mirroring edv `bracket_utils.split_u9_u11_into_pools`:
///   1. **weight spread (hard)** — `max_spread > 0`: cut into clusters whose
///      heaviest−lightest ≤ max_spread (greedy from the lightest; inclusive).
///   2. **pool size (soft balance)** — within each cluster, ceil(len/pool_size)
///      pools, sizes differing by ≤1 (5 @ size 4 → 3+2, never 4+1).
/// `max_spread ≤ 0` ⇒ one cluster (pure pool-size split). Pools numbered across
/// clusters in input (weight) order.
pub fn split_into_pools(weights_sorted: &[f64], pool_size: usize, max_spread: f64) -> Vec<Vec<usize>> {
    let n = weights_sorted.len();
    if n == 0 || pool_size == 0 {
        return Vec::new();
    }
    // Stage 1: clusters of consecutive indices within the spread.
    let mut clusters: Vec<(usize, usize)> = Vec::new(); // (start, end-exclusive)
    if max_spread > 0.0 {
        let mut start = 0;
        for i in 1..n {
            if weights_sorted[i] - weights_sorted[start] > max_spread {
                clusters.push((start, i));
                start = i;
            }
        }
        clusters.push((start, n));
    } else {
        clusters.push((0, n));
    }
    // Stage 2: even distribution by pool_size within each cluster.
    let mut pools: Vec<Vec<usize>> = Vec::new();
    for (start, end) in clusters {
        let len = end - start;
        let n_pools = len.div_ceil(pool_size);
        let base = len / n_pools;
        let extra = len % n_pools; // first `extra` pools get base+1
        let mut idx = start;
        for p in 0..n_pools {
            let size = if p < extra { base + 1 } else { base };
            pools.push((idx..idx + size).collect());
            idx += size;
        }
    }
    pools
}

/// Single-pool round-robin fight order (flattened, one bout per step). Avoids
/// back-to-back bouts for the same fighter. 2er = best-of-three (same pair ×3).
/// Indices are 0-based fighter positions. Mirror of edv `_generate_fight_schedule`.
pub fn pool_fight_schedule(pool_size: usize) -> Vec<(usize, usize)> {
    match pool_size {
        0 | 1 => vec![],
        2 => vec![(0, 1), (0, 1), (0, 1)],
        3 => vec![(0, 2), (1, 2), (0, 1)],
        4 => vec![(0, 3), (1, 2), (0, 2), (1, 3), (0, 1), (2, 3)],
        5 => vec![
            (0, 3), (1, 4), (0, 2), (1, 3), (2, 4),
            (0, 1), (2, 3), (0, 4), (1, 2), (3, 4),
        ],
        _ => circle_method(pool_size), // ≥6 (not used in practice; pools are ≤5)
    }
}

/// Doppelpool fight numbering: 2-by-2 interleaved across the two pools
/// (A,A,B,B,A,A,…) so each pool gets cross-pool rest. Returns (pool_a_numbers,
/// pool_b_numbers). edv `pool_renderer._generate_fight_numbers_for_double_pool`.
pub fn double_pool_fight_numbers(count_a: usize, count_b: usize) -> (Vec<i32>, Vec<i32>) {
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let mut n = 1i32;
    while a.len() < count_a || b.len() < count_b {
        for _ in 0..2 {
            if a.len() < count_a {
                a.push(n);
                n += 1;
            }
        }
        for _ in 0..2 {
            if b.len() < count_b {
                b.push(n);
                n += 1;
            }
        }
    }
    (a, b)
}

/// Circle method for larger pools (edv fallback, lines 71-84).
fn circle_method(pool_size: usize) -> Vec<(usize, usize)> {
    let n = if pool_size % 2 == 0 { pool_size } else { pool_size + 1 };
    let mut players: Vec<usize> = (0..n).collect();
    let mut out = Vec::new();
    for _ in 0..(n - 1) {
        for i in 0..(n / 2) {
            let (p1, p2) = (players[i], players[n - 1 - i]);
            if p1 < pool_size && p2 < pool_size {
                out.push((p1, p2));
            }
        }
        // rotate: keep players[0], move last to front of the rest
        let mut next = vec![players[0], players[n - 1]];
        next.extend_from_slice(&players[1..n - 1]);
        players = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn type_thresholds_match_claudemd() {
        assert_eq!(recommend_bracket_type(2, false), "special");
        assert_eq!(recommend_bracket_type(5, false), "pools");
        assert_eq!(recommend_bracket_type(8, false), "double");
        assert_eq!(recommend_bracket_type(16, false), "ko");
        assert_eq!(recommend_bracket_type(32, false), "ko");
        assert_eq!(recommend_bracket_type(50, false), "repechage");
        assert_eq!(recommend_bracket_type(64, false), "repechage");
        assert_eq!(recommend_bracket_type(80, false), "special"); // deferred
        assert_eq!(recommend_bracket_type(40, true), "pools"); // U9/U11 override
    }

    #[test]
    fn pool_schedules_match_edv() {
        assert_eq!(pool_fight_schedule(2), vec![(0, 1), (0, 1), (0, 1)]);
        assert_eq!(pool_fight_schedule(3), vec![(0, 2), (1, 2), (0, 1)]);
        assert_eq!(
            pool_fight_schedule(4),
            vec![(0, 3), (1, 2), (0, 2), (1, 3), (0, 1), (2, 3)]
        );
        assert_eq!(
            pool_fight_schedule(5),
            vec![(0, 3), (1, 4), (0, 2), (1, 3), (2, 4), (0, 1), (2, 3), (0, 4), (1, 2), (3, 4)]
        );
    }

    #[test]
    fn double_pool_numbering_interleaves_2by2() {
        // Two 4er pools (6 fights each): A=1,2,5,6,9,10 · B=3,4,7,8,11,12.
        let (a, b) = double_pool_fight_numbers(6, 6);
        assert_eq!(a, vec![1, 2, 5, 6, 9, 10]);
        assert_eq!(b, vec![3, 4, 7, 8, 11, 12]);
        // Uneven (4er pool A = 6 fights, 3er pool B = 3): B runs out, A continues.
        let (a, b) = double_pool_fight_numbers(6, 3);
        assert_eq!(b, vec![3, 4, 7]);
        assert_eq!(a.len(), 6);
        assert_eq!(a.iter().chain(b.iter()).copied().collect::<std::collections::BTreeSet<_>>().len(), 9);
    }

    #[test]
    fn schedules_are_complete_round_robins() {
        // Every pair meets exactly once (2er is the best-of-three exception).
        for n in 3..=8 {
            let sched = pool_fight_schedule(n);
            let pairs: BTreeSet<(usize, usize)> =
                sched.iter().map(|&(a, b)| if a < b { (a, b) } else { (b, a) }).collect();
            assert_eq!(pairs.len(), n * (n - 1) / 2, "n={n}: every pair once");
            assert_eq!(sched.len(), n * (n - 1) / 2, "n={n}: no extra/duplicate bouts");
            assert!(sched.iter().all(|&(a, b)| a < n && b < n && a != b), "n={n}: valid indices");
        }
    }

    #[test]
    fn split_into_pools_matches_edv_examples() {
        // edv doc example: pool_size=4, spread=5, weights 18/20/21/23/28/30/31
        // → Pool1 [18,20,21,23] (spread 5), Pool2 [28,30,31] (spread 3).
        let w = [18.0, 20.0, 21.0, 23.0, 28.0, 30.0, 31.0];
        assert_eq!(split_into_pools(&w, 4, 5.0), vec![vec![0, 1, 2, 3], vec![4, 5, 6]]);

        // 5 fighters within one spread, pool_size 4 → 3 + 2 (even), never 4 + 1.
        let w5 = [18.0, 19.0, 20.0, 21.0, 22.0];
        assert_eq!(split_into_pools(&w5, 4, 10.0), vec![vec![0, 1, 2], vec![3, 4]]);

        // spread = 0 (off) ⇒ one cluster, pure pool-size split (8 @ 4 → 4+4).
        let w8: Vec<f64> = (0..8).map(|i| 20.0 + i as f64).collect();
        assert_eq!(split_into_pools(&w8, 4, 0.0), vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7]]);

        // a genuine outlier becomes its own 1-fighter pool (solo).
        let wo = [20.0, 21.0, 40.0];
        assert_eq!(split_into_pools(&wo, 4, 5.0), vec![vec![0, 1], vec![2]]);

        // every fighter lands in exactly one pool, order preserved.
        for spread in [0.0, 3.0, 7.0] {
            let pools = split_into_pools(&w, 3, spread);
            let flat: Vec<usize> = pools.iter().flatten().copied().collect();
            assert_eq!(flat, (0..w.len()).collect::<Vec<_>>(), "spread={spread}: partition");
        }
    }
}
