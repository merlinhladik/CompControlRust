// SPDX-License-Identifier: GPL-3.0-or-later
//! Modified Doppel-KO loser bracket (half-sided repechage, TWO bronze) — pure.
//!
//! Frozen mirror of JF main.py `_LB_STRUCTURE[3]` (8er) and `[4]` (16er), which
//! is itself the machine-checked decode of the official DJB sheets
//! `edv/.../templates/ko_{8,16}.xls` (oracle: edv/tests/ko_form_decoder.py +
//! test_ko_form_order.py). The 32er (num_rounds=5) uses a DIFFERENT medal
//! topology and is deliberately NOT wired here (needs Merlin's sheet check) —
//! callers get None and stay graceful, exactly like JF.
//!
//! Indexing: num_rounds = log2(draw) (8er=3, 16er=4). slot 1 = participant1_id.
//! WB losers enter CROSSED (Judo-cross) — the cross edges below are the only
//! non-geometric part and the whole point of the frozen table.

use crate::ko::Slot;

fn slot_by_parity(pos: i32) -> Slot {
    if pos % 2 == 0 {
        Slot::P1
    } else {
        Slot::P2
    }
}

/// Where the WB loser of (wb_round, pos) drops into the LB. None = WB final
/// (no drop) or unwired size. Mirrors `_LB_STRUCTURE[n]["wb_drop"]`.
pub fn wb_drop(num_rounds: u32, wb_round: i32, pos: i32) -> Option<(i32, i32, Slot)> {
    match num_rounds {
        3 => match wb_round {
            // 8er: R0-loser → lb r0; SF-loser → bronze (lb r1), CROSS (1-p).
            0 => Some((0, pos / 2, slot_by_parity(pos))),
            1 => Some((1, 1 - pos, Slot::P2)),
            _ => None,
        },
        4 => match wb_round {
            // 16er: R0 → lb r0; QF → lb r1 CROSS (p XOR 2); SF → bronze (no cross).
            0 => Some((0, pos / 2, slot_by_parity(pos))),
            1 => Some((1, pos ^ 2, Slot::P2)),
            2 => Some((3, pos, Slot::P2)),
            _ => None,
        },
        _ => None, // 32er + unwired sizes: graceful, like JF
    }
}

/// Where the LB winner of (lb_round, pos) advances. None = already the bronze
/// match (caller finalizes) or unwired. Mirrors `_LB_STRUCTURE[n]["lb_advance"]`.
pub fn lb_advance(num_rounds: u32, lb_round: i32, pos: i32) -> Option<(i32, i32, Slot)> {
    match num_rounds {
        3 => match lb_round {
            0 => Some((1, pos, Slot::P1)), // lb r0 winner → bronze, slot 1
            _ => None,
        },
        4 => match lb_round {
            0 => Some((1, pos, Slot::P1)),               // → lb r1, slot 1
            1 => Some((2, pos / 2, slot_by_parity(pos))), // → lb r2 (merge)
            2 => Some((3, pos, Slot::P1)),               // → bronze, slot 1
            _ => None,
        },
        _ => None,
    }
}

/// Last LB round = the two bronze matches. None = unwired size.
pub fn bronze_round(num_rounds: u32) -> Option<i32> {
    match num_rounds {
        3 => Some(1),
        4 => Some(3),
        _ => None,
    }
}

/// Number of LB fights per LB round (for eager creation / iteration).
/// Mirrors `_LB_ROUND_SIZES`. 8er: (2,2); 16er: (4,4,2,2).
pub fn lb_round_sizes(num_rounds: u32) -> &'static [i32] {
    match num_rounds {
        3 => &[2, 2],
        4 => &[4, 4, 2, 2],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: the documented edges in WSP/CLAUDE.md (machine-checked vs ko_8/16.xls).

    #[test]
    fn eer8_edges() {
        // 8er: lb r0 p = L(R0 2p) vs L(R0 2p+1); bronze lb r1 p = W(lb r0 p) vs L(SF 1-p) CROSS.
        assert_eq!(wb_drop(3, 0, 0), Some((0, 0, Slot::P1)));
        assert_eq!(wb_drop(3, 0, 1), Some((0, 0, Slot::P2)));
        assert_eq!(wb_drop(3, 0, 2), Some((0, 1, Slot::P1)));
        assert_eq!(wb_drop(3, 0, 3), Some((0, 1, Slot::P2)));
        // SF (wb round 1) losers cross into bronze: p=0 → bronze pos 1, p=1 → bronze pos 0.
        assert_eq!(wb_drop(3, 1, 0), Some((1, 1, Slot::P2)));
        assert_eq!(wb_drop(3, 1, 1), Some((1, 0, Slot::P2)));
        // lb r0 winners go straight to the bronze match at same pos, slot 1.
        assert_eq!(lb_advance(3, 0, 0), Some((1, 0, Slot::P1)));
        assert_eq!(lb_advance(3, 0, 1), Some((1, 1, Slot::P1)));
        assert_eq!(bronze_round(3), Some(1));
        assert_eq!(lb_advance(3, 1, 0), None); // bronze match → finalize
    }

    #[test]
    fn eer16_edges() {
        // 16er: lb r1 p = W(lb r0 p) vs L(QF p^2) — CROSS; bronze lb r3 p vs L(SF p) no cross.
        assert_eq!(wb_drop(4, 0, 5), Some((0, 2, Slot::P2)));
        // QF (wb round 1) cross: p XOR 2.
        assert_eq!(wb_drop(4, 1, 0), Some((1, 2, Slot::P2)));
        assert_eq!(wb_drop(4, 1, 1), Some((1, 3, Slot::P2)));
        assert_eq!(wb_drop(4, 1, 2), Some((1, 0, Slot::P2)));
        assert_eq!(wb_drop(4, 1, 3), Some((1, 1, Slot::P2)));
        // SF (wb round 2) → bronze (lb r3), NO cross.
        assert_eq!(wb_drop(4, 2, 0), Some((3, 0, Slot::P2)));
        assert_eq!(wb_drop(4, 2, 1), Some((3, 1, Slot::P2)));
        // lb advances: r0→r1 slot1, r1→r2 merge, r2→bronze slot1.
        assert_eq!(lb_advance(4, 0, 2), Some((1, 2, Slot::P1)));
        assert_eq!(lb_advance(4, 1, 0), Some((2, 0, Slot::P1)));
        assert_eq!(lb_advance(4, 1, 1), Some((2, 0, Slot::P2)));
        assert_eq!(lb_advance(4, 1, 3), Some((2, 1, Slot::P2)));
        assert_eq!(lb_advance(4, 2, 1), Some((3, 1, Slot::P1)));
        assert_eq!(bronze_round(4), Some(3));
        assert_eq!(lb_advance(4, 3, 0), None);
    }

    #[test]
    fn unwired_sizes_are_graceful() {
        assert_eq!(wb_drop(5, 0, 0), None); // 32er not wired
        assert_eq!(bronze_round(5), None);
        assert_eq!(lb_round_sizes(5), &[] as &[i32]);
    }
}
