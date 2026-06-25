// SPDX-License-Identifier: GPL-3.0-or-later
//! Excel / CSV I/O — the highest-risk surface of the whole port.
//!
//! Python provenance:
//!   edv/backend/services/excel_form_filler.py     -> form_filler.rs
//!   edv/backend/services/excel_seeding_reimport.py -> seeding_reimport.rs
//!   edv/backend/services/urkunden_export_service.py -> urkunden.rs
//!   edv/backend/services/contestants_csv.py        -> contestants.rs
//!
//! WARNING: the DJB `ko_8|16|32.xls` topology sheets are BIFF (.xls). Python can
//! only read them with xlrd 1.2. `calamine` reads BIFF, but VERIFY round-trip
//! fidelity before trusting it. Fallback strategy (PLAN.md): keep a thin Python
//! sidecar for Excel until calamine parity is proven against the oracle suite.

pub mod config;        // bracket_config.xlsx (age/weight classification)
pub mod contestants;   // contestants_*.csv / .json  (lowest risk — start here)
pub mod export;        // generated .xlsx / .csv output (results, Urkunden, Wiegekarten)
pub mod pdf;           // printable, hand-fillable PDF output
pub mod print_html;    // printable HTML (CSS paged-media) — weigh-in cards
pub mod form_filler;   // DJB bracket/pool forms
pub mod urkunden;      // certificates
pub mod seeding_reimport;
