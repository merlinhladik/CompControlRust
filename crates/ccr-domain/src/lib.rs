// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure tournament domain logic — no I/O, no DB, no HTTP.
//!
//! This crate is the highest-risk and highest-value port target. Every module
//! here mirrors a Python source whose behaviour is pinned by tests that MUST be
//! ported alongside the code (see PLAN.md, Phase 4).
//!
//! Provenance map (Python -> Rust):
//!   edv/frontend/utils/pool_renderer.py          -> pools.rs (fight schedule)
//!   edv/utils/bracket_utils.py                   -> ko.rs    (balanced KO seeding)
//!   edv/tests/ko_form_decoder.py + _LB_STRUCTURE -> doppel_ko.rs
//!   edv/tests/repechage_form_decoder.py          -> repechage.rs
//!   JF main.py:_compute_pool_standings           -> standings.rs (DJB tiebreaker)

pub mod jvp;
pub mod pools;
pub mod ko;
pub mod ko32;
pub mod ko_big;
pub mod doppel_ko;
pub mod repechage;
pub mod standings;

// TODO(Phase 4): port the frozen topology tables verbatim and re-run the
// edv/tests oracle suite against this crate. Until then these modules are stubs.
