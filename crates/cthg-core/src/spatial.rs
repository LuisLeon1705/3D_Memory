//! The X axis as a tree: how strongly a memory anchored at one path applies to a path being
//! worked on. This is what lets recall work with zero useful words in the prompt ("arréglalo"):
//! the files touched are the query.
//!
//! affinity(anchor → target) = λ^d · σ
//! - d: levels from the anchor down to the target (0 = same file). A rule on `src/core` applies
//!   to `src/core/collider.rs` with λ¹, to `src/core/a/b.rs` with λ².
//! - σ: 1, or `OTHER_SYMBOL` when the anchor names a symbol and the target's known symbols
//!   (diff hunks) don't include it.
//! - An anchor that is not the target or one of its ancestors: 0. An empty anchor (the root) is
//!   not spatial at all: cross-cutting rules must be `CORE`.

use super::model::Memory;

pub const LAMBDA: f32 = 0.8;
pub const OTHER_SYMBOL: f32 = 0.5;

/// A path being worked on, and how sure we are that it matters now.
#[derive(Clone, Debug)]
pub struct Target {
    pub path: String,
    /// Symbols touched inside it (from diff hunk headers), if known.
    pub symbols: Vec<String>,
    /// 1.0 just read by the agent · 0.8 modified · 0.5 untracked · lower for noise.
    pub weight: f32,
}

impl Target {
    pub fn new(path: &str, weight: f32) -> Self {
        Target { path: normalize(path), symbols: vec![], weight }
    }
}

/// `/`-separated, relative, no `./`, no trailing slash. `\` becomes `/`.
pub fn normalize(p: &str) -> String {
    let p = p.trim().replace('\\', "/");
    let mut s = p.as_str();
    loop {
        let t = s.trim_start_matches("./").trim_start_matches('/');
        if t.len() == s.len() {
            break;
        }
        s = t;
    }
    s.trim_end_matches('/').to_string()
}

pub fn depth(p: &str) -> usize {
    if p.is_empty() {
        0
    } else {
        p.split('/').count()
    }
}

/// `dir` is `p` itself or one of its ancestors (the root "" is everyone's ancestor).
pub fn is_ancestor_or_self(dir: &str, p: &str) -> bool {
    dir.is_empty() || p == dir || (p.len() > dir.len() && p.starts_with(dir) && p.as_bytes()[dir.len()] == b'/')
}

/// The path and every ancestor up to the root: `src/core/a.rs` → `src/core/a.rs`, `src/core`, `src`, "".
pub fn ancestors(p: &str) -> Vec<String> {
    let mut out = vec![p.to_string()];
    let mut cur = p;
    while let Some(i) = cur.rfind('/') {
        cur = &cur[..i];
        out.push(cur.to_string());
    }
    if !p.is_empty() {
        out.push(String::new());
    }
    out
}

pub fn affinity(anchor: &str, anchor_symbol: &str, target: &Target) -> f32 {
    if anchor.is_empty() || !is_ancestor_or_self(anchor, &target.path) {
        return 0.0;
    }
    let d = depth(&target.path) - depth(anchor);
    let mut a = LAMBDA.powi(d as i32);
    if d == 0 && !anchor_symbol.is_empty() && !target.symbols.is_empty() && !target.symbols.iter().any(|s| s == anchor_symbol) {
        a *= OTHER_SYMBOL;
    }
    a
}

/// Spatial seed per node: the best weighted affinity over all targets.
pub fn seeds(m: &Memory, targets: &[Target]) -> Vec<f32> {
    m.nodes
        .iter()
        .map(|n| {
            targets
                .iter()
                .map(|t| t.weight * affinity(&n.path, &n.symbol, t))
                .fold(0.0f32, f32::max)
        })
        .collect()
}

/// A file or directory moved: every memory anchored at `from` or below follows it.
/// Returns how many nodes moved. (Git reports renames with `git diff -M` / `git log --follow`.)
pub fn rename(m: &mut Memory, from: &str, to: &str) -> usize {
    let (from, to) = (normalize(from), normalize(to));
    if from.is_empty() {
        return 0;
    }
    let mut moved = 0;
    for n in m.nodes.iter_mut() {
        if is_ancestor_or_self(&from, &n.path) {
            n.path = format!("{to}{}", &n.path[from.len()..]);
            moved += 1;
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_inheritance() {
        let t = Target::new(".\\src\\core\\collider.rs", 1.0);
        assert_eq!(t.path, "src/core/collider.rs");
        assert_eq!(affinity("src/core/collider.rs", "", &t), 1.0);
        assert!((affinity("src/core", "", &t) - LAMBDA).abs() < 1e-6);
        assert!((affinity("src", "", &t) - LAMBDA * LAMBDA).abs() < 1e-6);
        assert_eq!(affinity("src/co", "", &t), 0.0, "prefix of a name is not an ancestor");
        assert_eq!(affinity("src/ui", "", &t), 0.0);
        assert_eq!(affinity("", "", &t), 0.0, "root is not spatial: use CORE");
        assert_eq!(ancestors("src/core/a.rs"), vec!["src/core/a.rs", "src/core", "src", ""]);
    }

    #[test]
    fn symbols_narrow_the_file() {
        let mut t = Target::new("src/core/collider.rs", 1.0);
        t.symbols = vec!["resolve_contacts".into()];
        assert_eq!(affinity("src/core/collider.rs", "resolve_contacts", &t), 1.0);
        assert_eq!(affinity("src/core/collider.rs", "broad_phase", &t), OTHER_SYMBOL);
    }
}
