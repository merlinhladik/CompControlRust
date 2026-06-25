// SPDX-License-Identifier: GPL-3.0-or-later
//! Judo-Verband Pfalz "Additionssystem bis 20 Punkte" (WKO § 3.5, in force
//! 2025-03-23) — the additive cumulative youth ruleset for U9/U11.
//!
//! Point values (exact, do not re-derive — WKO § 3.5):
//!   Ippon = 10, Waza-ari = 5, Yuko = 3, Shido = +2 **to the opponent**.
//! First fighter to reach **≥ 20 = win (Sore Made)**; equal totals at time =
//! **Hiki-wake** (draw — there is NO golden score in this system).
//!
//! Ipponboard owns the *live* JVP rules (hold-time caps, §3.5.3 sequence caps).
//! CCR only needs the additive total + outcome from the resulting sub-score
//! counts, both for its own native scoring and for the values the webhook
//! delivers. Pure logic, no I/O.

pub const IPPON: i32 = 10;
pub const WAZARI: i32 = 5;
pub const YUKO: i32 = 3;
pub const SHIDO: i32 = 2; // awarded to the OPPONENT
pub const TARGET: i32 = 20;

/// One fighter's raw sub-score counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubScores {
    pub ippon: i32,
    pub wazari: i32,
    pub yuko: i32,
    pub shido: i32,
}

impl SubScores {
    pub fn new(ippon: i32, wazari: i32, yuko: i32, shido: i32) -> Self {
        Self { ippon, wazari, yuko, shido }
    }
}

/// Additive JVP total for `own`, including the +2/Shido the `opponent` collected
/// (a Shido scores points for the *other* fighter, never escalating to HSM).
pub fn total(own: &SubScores, opponent: &SubScores) -> i32 {
    own.ippon * IPPON + own.wazari * WAZARI + own.yuko * YUKO + opponent.shido * SHIDO
}

/// Outcome of a JVP fight given both fighters' sub-scores and whether regular
/// time has elapsed. Before time: only a ≥20 lead decides (Sore Made); otherwise
/// the fight is still `Ongoing`. At time: higher total wins, equal = `Draw`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Fighter1,
    Fighter2,
    Draw,
    Ongoing,
}

pub fn outcome(s1: &SubScores, s2: &SubScores, time_up: bool) -> Outcome {
    let t1 = total(s1, s2);
    let t2 = total(s2, s1);
    // Sore Made: a fighter reaching the target with a strict lead wins immediately.
    if t1 >= TARGET && t1 > t2 {
        return Outcome::Fighter1;
    }
    if t2 >= TARGET && t2 > t1 {
        return Outcome::Fighter2;
    }
    if !time_up {
        return Outcome::Ongoing;
    }
    match t1.cmp(&t2) {
        std::cmp::Ordering::Greater => Outcome::Fighter1,
        std::cmp::Ordering::Less => Outcome::Fighter2,
        std::cmp::Ordering::Equal => Outcome::Draw, // Hiki-wake
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_values_match_wko_3_5() {
        // One of each, no opponent shido.
        let s = SubScores::new(1, 1, 1, 0);
        assert_eq!(total(&s, &SubScores::default()), 10 + 5 + 3);
    }

    #[test]
    fn shido_scores_for_the_opponent() {
        // fighter2 has 2 shido ⇒ +4 for fighter1.
        let f1 = SubScores::new(0, 0, 0, 0);
        let f2 = SubScores::new(0, 0, 0, 2);
        assert_eq!(total(&f1, &f2), 4);
        assert_eq!(total(&f2, &f1), 0);
    }

    #[test]
    fn sore_made_at_twenty_wins_before_time() {
        // 2x Ippon = 20 ⇒ immediate win even with time left.
        let f1 = SubScores::new(2, 0, 0, 0);
        let f2 = SubScores::default();
        assert_eq!(outcome(&f1, &f2, false), Outcome::Fighter1);
    }

    #[test]
    fn under_twenty_is_ongoing_until_time() {
        let f1 = SubScores::new(1, 1, 0, 0); // 15
        let f2 = SubScores::new(0, 0, 1, 0); // 3
        assert_eq!(outcome(&f1, &f2, false), Outcome::Ongoing);
        assert_eq!(outcome(&f1, &f2, true), Outcome::Fighter1);
    }

    #[test]
    fn equal_at_time_is_hiki_wake() {
        let f1 = SubScores::new(0, 1, 1, 0); // 8
        let f2 = SubScores::new(0, 1, 1, 0); // 8
        assert_eq!(outcome(&f1, &f2, false), Outcome::Ongoing);
        assert_eq!(outcome(&f1, &f2, true), Outcome::Draw);
    }

    #[test]
    fn tie_at_twenty_needs_time_to_be_a_draw() {
        // Both reach 20 (no strict lead) ⇒ not decided until time, then draw.
        let f1 = SubScores::new(2, 0, 0, 0);
        let f2 = SubScores::new(2, 0, 0, 0);
        assert_eq!(outcome(&f1, &f2, false), Outcome::Ongoing);
        assert_eq!(outcome(&f1, &f2, true), Outcome::Draw);
    }
}
