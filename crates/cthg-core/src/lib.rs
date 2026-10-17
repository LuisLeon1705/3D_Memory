//! CTHG — Causal-Temporal HyperGraph: a project memory with three axes.
//!
//! - X: where it applies — a path in the repository tree (plus an optional symbol).
//! - Y: when it was learned — Unix time, forward-only.
//! - Z: how far to trust it — χ in [−1, 1], computed from structure and evidence (`score`).

pub mod binary;
pub mod hook;
pub mod import;
pub mod learn;
pub mod model;
pub mod retrieve;
pub mod score;
pub mod spatial;
pub mod text;
pub mod verify;
