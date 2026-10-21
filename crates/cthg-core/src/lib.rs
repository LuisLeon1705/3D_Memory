//! CTHG — Causal-Temporal HyperGraph: a project memory with three axes.
//!
//! - X: where it applies — a path in the repository tree (plus an optional symbol).
//! - Y: when it was learned — Unix time, forward-only.
//! - Z: how far to trust it — χ in [−1, 1], computed from structure and evidence (`score`).

pub mod antigravity;
pub mod binary;
pub mod correction;
pub mod eval;
pub mod hook;
pub mod import;
pub mod index;
pub mod json;
pub mod lang;
pub mod lang_html;
pub mod lang_js;
pub mod lang_python;
pub mod lang_rust;
pub mod learn;
pub mod model;
pub mod retrieve;
pub mod score;
pub mod spatial;
pub mod text;
pub mod verify;
