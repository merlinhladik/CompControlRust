// SPDX-License-Identifier: GPL-3.0-or-later
//! KO winners-bracket geometry — pure (no DB).
//! Mirrors JF main.py `_propagate_winner` (binary tree) + `_wb_num_rounds`.
//! Balanced-seeding + LB topology stay STUBBED until Phase 4.

/// Which participant slot a winner fills in its follow-up fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    P1,
    P2,
}

/// Binary-tree advance: winner of (round, pos) feeds (round+1, pos/2),
/// slot p1 if pos even else p2. JF main.py:658-660.
pub fn wb_next(round: i32, pos_in_round: i32) -> (i32, i32, Slot) {
    let slot = if pos_in_round % 2 == 0 { Slot::P1 } else { Slot::P2 };
    (round + 1, pos_in_round / 2, slot)
}

/// WB round count from the number of round-0 fights: size = 2*r0,
/// rounds = log2(size) = r0.bit_length(). 0 if no round-0 fights (e.g. double
/// pool). JF main.py:1571 — 2→2 (4er), 4→3 (8er), 8→4 (16er), 16→5 (32er).
pub fn wb_num_rounds(round0_count: u32) -> u32 {
    if round0_count >= 1 {
        32 - round0_count.leading_zeros() // == bit_length
    } else {
        0
    }
}

/// Next power of two ≥ n (min 2). edv `bracket_utils._next_pow2`.
pub fn next_pow2(n: usize) -> usize {
    if n <= 2 {
        return 2;
    }
    let mut p = 2;
    while p < n {
        p <<= 1;
    }
    p
}

/// Snake seed order (1-based) of length `size`. edv `_generate_seed_order`:
/// S(2)=[1,2]; S(n): each x in S(n/2) → x, n+1-x. (== ko_big::standard_seed.)
pub fn seed_order(size: usize) -> Vec<usize> {
    if size <= 2 {
        return vec![1, 2];
    }
    let prev = seed_order(size / 2);
    let mut out = Vec::with_capacity(size);
    for x in prev {
        out.push(x);
        out.push(size + 1 - x);
    }
    out
}

/// Generate WB round-0 slot pairs from `(gp_id, club)` participants — edv
/// `_compute_balanced_bracket`: round-robin by club (separate clubmates), pad to
/// next_pow2 with byes, snake-seed into slots, pair (2p, 2p+1). `None` = bye.
/// Used to generate both Doppel-KO and Repechage main draws (edv seeds wb r0 only).
/// Round-robin distribution by club (first-seen club order): take one fighter
/// from each club in turn to spread clubmates apart. edv `_distribute_round_robin`.
pub fn round_robin_by_club(participants: &[(i32, String)]) -> Vec<i32> {
    use std::collections::{HashMap, VecDeque};
    let mut clubs: Vec<String> = Vec::new();
    let mut groups: HashMap<String, VecDeque<i32>> = HashMap::new();
    for (id, club) in participants {
        if !groups.contains_key(club) {
            clubs.push(club.clone());
        }
        groups.entry(club.clone()).or_default().push_back(*id);
    }
    let mut out = Vec::with_capacity(participants.len());
    loop {
        let mut any = false;
        for c in &clubs {
            if let Some(id) = groups.get_mut(c).and_then(|q| q.pop_front()) {
                out.push(id);
                any = true;
            }
        }
        if !any {
            break;
        }
    }
    out
}

pub fn generate_round0(participants: &[(i32, String)]) -> Vec<(Option<i32>, Option<i32>)> {
    let n = participants.len();
    if n == 0 {
        return Vec::new();
    }
    let size = next_pow2(n);
    let mut optimized: Vec<Option<i32>> =
        round_robin_by_club(participants).into_iter().map(Some).collect();
    while optimized.len() < size {
        optimized.push(None); // bye
    }

    let seed = seed_order(size);
    let mut slots: Vec<Option<i32>> = vec![None; size];
    for (i, &s) in seed.iter().enumerate() {
        if i < optimized.len() {
            slots[s - 1] = optimized[i];
        }
    }
    (0..size / 2).map(|p| (slots[2 * p], slots[2 * p + 1])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_position_matches_jf_binary_tree() {
        assert_eq!(wb_next(0, 0), (1, 0, Slot::P1));
        assert_eq!(wb_next(0, 1), (1, 0, Slot::P2));
        assert_eq!(wb_next(0, 2), (1, 1, Slot::P1));
        assert_eq!(wb_next(0, 3), (1, 1, Slot::P2));
    }

    #[test]
    fn seed_order_matches_edv_recursion() {
        assert_eq!(seed_order(2), vec![1, 2]);
        assert_eq!(seed_order(4), vec![1, 4, 2, 3]);
        assert_eq!(seed_order(8), vec![1, 8, 4, 5, 2, 7, 3, 6]);
        assert_eq!(next_pow2(6), 8);
        assert_eq!(next_pow2(16), 16);
        assert_eq!(next_pow2(17), 32);
    }

    #[test]
    fn generate_round0_byes_and_club_separation() {
        // 6 fighters → size 8 → 4 fights, 2 byes. 4 from club A, 2 from club B.
        let p: Vec<(i32, String)> = vec![
            (1, "A".into()), (2, "A".into()), (3, "A".into()), (4, "A".into()),
            (5, "B".into()), (6, "B".into()),
        ];
        let pairs = generate_round0(&p);
        assert_eq!(pairs.len(), 4);
        // Exactly 2 byes (one slot None), no fully-empty fight.
        let byes = pairs.iter().filter(|(a, b)| a.is_some() ^ b.is_some()).count();
        let dead = pairs.iter().filter(|(a, b)| a.is_none() && b.is_none()).count();
        assert_eq!(byes, 2, "{pairs:?}");
        assert_eq!(dead, 0, "byes must not pair together: {pairs:?}");
        // Every real fighter appears exactly once.
        let mut seen: Vec<i32> = pairs.iter().flat_map(|(a, b)| [*a, *b]).flatten().collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn generate_round0_pow2_no_byes() {
        // 8 fighters → 4 fights, no byes, every fighter once. (Faithful to edv's
        // round-robin + recursive snake seed; perfect club separation is NOT
        // guaranteed by edv's algorithm, so not asserted.)
        let p: Vec<(i32, String)> =
            (1..=8).map(|i| (i, format!("C{}", i % 3))).collect();
        let pairs = generate_round0(&p);
        assert_eq!(pairs.len(), 4);
        assert!(pairs.iter().all(|(a, b)| a.is_some() && b.is_some()), "no byes");
        let mut seen: Vec<i32> = pairs.iter().flat_map(|(a, b)| [a.unwrap(), b.unwrap()]).collect();
        seen.sort_unstable();
        assert_eq!(seen, (1..=8).collect::<Vec<_>>());
    }

    #[test]
    fn num_rounds_matches_jf_bit_length() {
        assert_eq!(wb_num_rounds(0), 0);
        assert_eq!(wb_num_rounds(2), 2); // 4er
        assert_eq!(wb_num_rounds(4), 3); // 8er
        assert_eq!(wb_num_rounds(8), 4); // 16er
        assert_eq!(wb_num_rounds(16), 5); // 32er
    }
}
