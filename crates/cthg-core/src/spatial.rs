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
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub path: String,
    /// Symbols touched inside it (from diff hunk headers), if known.
    pub symbols: Vec<String>,
    /// 1.0 just read by the agent · 0.8 modified · 0.5 untracked · lower for noise.
    pub weight: f32,
    /// Changed lines (`deleted + added`) from `git diff`, used to rank dirty worktrees.
    pub diff_lines: usize,
}

impl Target {
    pub fn new(path: &str, weight: f32) -> Self {
        Target { path: normalize(path), symbols: vec![], weight, diff_lines: 0 }
    }
}

/// Summary of one file's changes in a unified `git diff`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub symbols: Vec<String>,
    pub lines: usize,
    /// First line of each hunk in the new file (`+N` of `@@ -a,b +N,M @@`).
    pub hunk_starts: Vec<usize>,
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
    if from.is_empty() || to.is_empty() || from == to {
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

/// Extracts `(old_path, new_path)` renames from `git status --porcelain=v1` and `git diff -M`.
pub fn parse_renames(status_porcelain: &str, diff: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push_pair = |from: &str, to: &str| {
        let (f, t) = (normalize(from.trim_matches('"')), normalize(to.trim_matches('"')));
        if !f.is_empty() && !t.is_empty() && f != t && !out.iter().any(|(a, b)| a == &f && b == &t) {
            out.push((f, t));
        }
    };

    for line in status_porcelain.lines().filter(|l| l.len() > 3) {
        let (code, rest) = line.split_at(3);
        if code.contains('R') {
            if let Some((from, to)) = rest.split_once(" -> ") {
                push_pair(from, to);
            }
        }
    }

    let mut pending_from: Option<&str> = None;
    for line in diff.lines() {
        if let Some(from) = line.strip_prefix("rename from ") {
            pending_from = Some(from.trim());
        } else if let Some(to) = line.strip_prefix("rename to ") {
            if let Some(from) = pending_from.take() {
                push_pair(from, to.trim());
            }
        } else if line.starts_with("diff --git ") {
            pending_from = None;
        }
    }
    out
}

/// Rewrites node paths for detected file and directory renames.
/// When `src/core/collider.rs` moved to `src/physics/collider.rs` and `src/core` no longer exists,
/// the parent directory `src/core` itself is renamed to `src/physics`.
pub fn apply_renames(m: &mut Memory, renames: &[(String, String)], path_exists: impl Fn(&str) -> bool) -> usize {
    let mut moved = 0;
    for (from, to) in renames {
        let (from, to) = (normalize(from), normalize(to));
        if let Some((from_dir, to_dir)) = common_suffix_dirs(&from, &to) {
            if !path_exists(&from_dir) {
                moved += rename(m, &from_dir, &to_dir);
            }
        }
        moved += rename(m, &from, &to);
    }
    moved
}

fn common_suffix_dirs(from: &str, to: &str) -> Option<(String, String)> {
    let mut f_parts: Vec<&str> = from.split('/').collect();
    let mut t_parts: Vec<&str> = to.split('/').collect();
    let mut stripped = 0;
    while f_parts.len() > 1 && t_parts.len() > 1 && f_parts.last() == t_parts.last() {
        f_parts.pop();
        t_parts.pop();
        stripped += 1;
    }
    if stripped == 0 {
        return None;
    }
    let (fd, td) = (f_parts.join("/"), t_parts.join("/"));
    (fd != td && !fd.is_empty() && !td.is_empty()).then_some((fd, td))
}

/// Parses a unified `git diff` into per-file line counts and touched symbols from `@@` headers.
pub fn parse_diff(diff: &str) -> Vec<FileDiff> {
    let mut out: Vec<FileDiff> = Vec::new();
    let mut cur_idx: Option<usize> = None;

    let ensure_file = |out: &mut Vec<FileDiff>, path: &str| -> Option<usize> {
        let p = normalize(path.trim_matches('"'));
        if p.is_empty() || p == "dev/null" {
            return None;
        }
        if let Some(i) = out.iter().position(|f| f.path == p) {
            Some(i)
        } else {
            out.push(FileDiff { path: p, symbols: Vec::new(), lines: 0, hunk_starts: Vec::new() });
            Some(out.len() - 1)
        }
    };

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            cur_idx = rest
                .rsplit_once(" b/")
                .map(|(_, b)| b)
                .and_then(|b| ensure_file(&mut out, b));
        } else if let Some(to) = line.strip_prefix("rename to ") {
            if let Some(i) = cur_idx {
                out[i].path = normalize(to.trim().trim_matches('"'));
            } else {
                cur_idx = ensure_file(&mut out, to.trim());
            }
        } else if let Some(plus) = line.strip_prefix("+++ b/") {
            if let Some(i) = cur_idx {
                out[i].path = normalize(plus.trim().trim_matches('"'));
            } else {
                cur_idx = ensure_file(&mut out, plus.trim());
            }
        } else if line.starts_with("@@ ") {
            if let Some(i) = cur_idx {
                out[i].lines += hunk_line_count(line);
                if let Some(start) = hunk_new_start(line) {
                    out[i].hunk_starts.push(start);
                }
                if let Some(sym) = extract_hunk_symbol(line) {
                    if !out[i].symbols.contains(&sym) {
                        out[i].symbols.push(sym);
                    }
                }
            }
        }
    }
    out
}

fn hunk_line_count(header: &str) -> usize {
    let Some(rest) = header.strip_prefix("@@ ") else { return 0 };
    let Some((ranges, _)) = rest.split_once(" @@") else { return 0 };
    let mut total = 0usize;
    for tok in ranges.split_whitespace() {
        if tok.starts_with('-') || tok.starts_with('+') {
            let count = match tok[1..].split_once(',') {
                Some((_, c)) => c.parse::<usize>().unwrap_or(0),
                None => 1,
            };
            total += count;
        }
    }
    total
}

fn hunk_new_start(header: &str) -> Option<usize> {
    let rest = header.strip_prefix("@@ ")?;
    let (ranges, _) = rest.split_once(" @@")?;
    let plus = ranges.split_whitespace().find_map(|t| t.strip_prefix('+'))?;
    plus.split(',').next()?.parse().ok()
}

/// Extracts the enclosing function or type name from a `@@ ... @@ <context>` hunk header.
pub fn extract_hunk_symbol(header: &str) -> Option<String> {
    let rest = header.strip_prefix("@@ ")?;
    let (_, ctx) = rest.split_once("@@")?;
    let ctx = ctx.trim();
    if ctx.is_empty() {
        return None;
    }
    if let Some(name) = declared_name(ctx) {
        return Some(name);
    }

    // 3. Fallback: identifier immediately before `(` (e.g. `int Collider::broad_phase(int n)`).
    if let Some((before_paren, _)) = ctx.split_once('(') {
        let last_word = before_paren.split_whitespace().last()?;
        let tail = last_word.rsplit("::").next().unwrap_or(last_word);
        if let Some(ident) = clean_ident(tail) {
            if !matches!(ident.as_str(), "if" | "for" | "while" | "match" | "switch" | "return" | "catch" | "sizeof") {
                return Some(ident);
            }
        }
    }
    None
}

/// Name declared on one line (`fn x`, `def x`, `class X`, `impl X`…), or `None`.
/// No call-like fallback: safe to use on arbitrary body lines.
fn declared_name(ctx: &str) -> Option<String> {
    // 1. Explicit declaration keywords: `fn`, `def`, `function`, `func`, `struct`, `enum`, `trait`, `class`, `interface`, `type`, `mod`.
    let tokens: Vec<&str> = ctx.split_whitespace().collect();
    for (i, &tok) in tokens.iter().enumerate() {
        let kw = tok.split('<').next().unwrap_or(tok);
        if matches!(kw, "fn" | "def" | "function" | "struct" | "enum" | "trait" | "class" | "interface" | "type" | "mod") {
            if let Some(next) = tokens.get(i + 1) {
                if let Some(ident) = clean_ident(next) {
                    return Some(ident);
                }
            }
        } else if kw == "func" {
            // Go methods may have a receiver `func (s *Server) Handle(...)`.
            let mut j = i + 1;
            if tokens.get(j).is_some_and(|t| t.starts_with('(')) {
                while j < tokens.len() && !tokens[j].contains(')') {
                    j += 1;
                }
                j += 1;
            }
            if let Some(next) = tokens.get(j) {
                if let Some(ident) = clean_ident(next) {
                    return Some(ident);
                }
            }
        }
    }

    // 2. Rust `impl` header when no `fn` is on the line: `impl<T> Trait for Type` or `impl<T> Type`.
    let is_impl = |t: &&str| *t == "impl" || t.starts_with("impl<");
    if tokens.iter().any(is_impl) {
        if let Some(pos) = tokens.iter().position(|&t| t == "for") {
            if let Some(next) = tokens.get(pos + 1).and_then(|t| clean_ident(t)) {
                return Some(next);
            }
        }
        if let Some(pos) = tokens.iter().position(is_impl) {
            if let Some(next) = tokens.get(pos + 1).and_then(|t| clean_ident(t)) {
                return Some(next);
            }
        }
    }

    None
}

/// The innermost declaration enclosing 1-based `line` of `source`: the closest line at or above
/// it that declares something. Git's default hunk header misses indented methods (it shows
/// `impl Collider {`), so the file itself is the reliable source.
pub fn enclosing_symbol(source: &str, line: usize) -> Option<String> {
    let above: Vec<&str> = source.lines().take(line.max(1)).collect();
    let is_comment = |l: &str| ["//", "#", "/*", "*", "--"].iter().any(|c| l.starts_with(c));
    above
        .iter()
        .map(|l| l.trim())
        .rev()
        .filter(|l| !is_comment(l))
        .find_map(declared_name)
}

/// Replaces hunk-header symbols with the declarations that really enclose each hunk, read from
/// the working tree. Header symbols are kept too, so type-level anchors (`Collider`) still match.
pub fn refine_symbols(targets: &mut [Target], diff: &str, read_file: impl Fn(&str) -> Option<String>) {
    let diffs = parse_diff(diff);
    for t in targets.iter_mut() {
        let Some(d) = diffs.iter().find(|d| d.path == t.path) else { continue };
        let Some(source) = read_file(&t.path) else { continue };
        let mut symbols: Vec<String> = d.hunk_starts.iter().filter_map(|&l| enclosing_symbol(&source, l)).collect();
        for s in std::mem::take(&mut t.symbols) {
            if !symbols.contains(&s) {
                symbols.push(s);
            }
        }
        symbols.dedup();
        t.symbols = symbols;
    }
}

/// Renames already committed (`git log -M --diff-filter=R --name-status --format=`), oldest first,
/// so a memory whose path vanished can follow the file before `verify` calls it deleted.
pub fn parse_log_renames(log: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = log
        .lines()
        .filter(|l| l.starts_with('R'))
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let _ = parts.next();
            Some((normalize(parts.next()?), normalize(parts.next()?)))
        })
        .filter(|(f, t)| !f.is_empty() && !t.is_empty() && f != t)
        .collect();
    out.reverse();
    out
}

fn clean_ident(raw: &str) -> Option<String> {
    let s = raw.trim_start_matches(['&', '*']);
    let ident: String = s.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    (!ident.is_empty() && ident.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')).then_some(ident)
}

/// Combines `git status --porcelain=v1` and `git diff -M -U0`, filters noise, attaches hunk
/// symbols, ranks modified files by diff size (`diff_lines`) and truncates to `max_paths`.
pub fn rank_git_targets(
    status_porcelain: &str,
    diff: &str,
    noise_files: &[&str],
    noise_dirs: &[&str],
    max_paths: usize,
) -> Vec<Target> {
    let diffs = parse_diff(diff);
    let is_noise = |path: &str| -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        noise_files.contains(&name)
            || noise_dirs
                .iter()
                .any(|d| path.starts_with(d) || path.contains(&format!("/{d}")))
    };

    let mut targets: Vec<Target> = Vec::new();
    for line in status_porcelain.lines().filter(|l| l.len() > 3) {
        let (code, rest) = line.split_at(3);
        let path = rest.rsplit(" -> ").next().unwrap_or(rest).trim_matches('"');
        let path = normalize(path);
        if path.is_empty() || is_noise(&path) || targets.iter().any(|t| t.path == path) {
            continue;
        }
        let weight = if code.starts_with("??") { 0.5 } else { 0.8 };
        let (symbols, diff_lines) = diffs
            .iter()
            .find(|d| d.path == path)
            .map(|d| (d.symbols.clone(), d.lines))
            .unwrap_or_default();
        targets.push(Target { path, symbols, weight, diff_lines });
    }

    for d in diffs {
        if !d.path.is_empty() && !is_noise(&d.path) && !targets.iter().any(|t| t.path == d.path) {
            targets.push(Target { path: d.path, symbols: d.symbols, weight: 0.8, diff_lines: d.lines });
        }
    }

    targets.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.diff_lines.cmp(&a.diff_lines))
    });
    targets.truncate(max_paths);
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::{learn, Claim};

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

    #[test]
    fn diff_hunks_extract_symbols_and_rank_paths_by_diff_size() {
        let status = " M src/small.rs\n M src/core/collider.rs\n?? src/scratch.rs\n M Cargo.lock\n";
        let diff = "\
diff --git a/src/small.rs b/src/small.rs
--- a/src/small.rs
+++ b/src/small.rs
@@ -4,1 +4,2 @@ pub fn tiny_helper() {
diff --git a/src/core/collider.rs b/src/core/collider.rs
--- a/src/core/collider.rs
+++ b/src/core/collider.rs
@@ -12,8 +12,20 @@ pub fn resolve_contacts(a: &Body, b: &Body) -> usize {
@@ -60,2 +72,6 @@ impl Collider {
";
        let targets = rank_git_targets(status, diff, &["Cargo.lock"], &["target/"], 12);
        assert_eq!(targets.len(), 3, "Cargo.lock is filtered out");
        assert_eq!(targets[0].path, "src/core/collider.rs", "largest diff ranks first");
        assert_eq!(targets[0].diff_lines, 36);
        assert_eq!(targets[0].symbols, vec!["resolve_contacts", "Collider"]);
        assert_eq!(targets[1].path, "src/small.rs");
        assert_eq!(targets[1].diff_lines, 3);
        assert_eq!(targets[1].symbols, vec!["tiny_helper"]);
        assert_eq!(targets[2].path, "src/scratch.rs", "untracked ranks after modified");
        assert_eq!(targets[2].weight, 0.5);
    }

    #[test]
    fn auto_renames_move_file_and_directory_memories() {
        let mut m = Memory::default();
        let dir_rule = learn(&mut m, &Claim::rule("Todo el módulo es determinista", "src/core"), 100.0)[0].node;
        let file_rule = learn(&mut m, &Claim::rule("No uses unwrap en el colisionador", "src/core/collider.rs"), 101.0)[0].node;

        let status = "R  src/core/collider.rs -> src/physics/collider.rs\n";
        let diff = "\
diff --git a/src/core/collider.rs b/src/physics/collider.rs
similarity index 96%
rename from src/core/collider.rs
rename to src/physics/collider.rs
";
        let renames = parse_renames(status, diff);
        assert_eq!(renames, vec![("src/core/collider.rs".into(), "src/physics/collider.rs".into())]);

        // `src/core` no longer exists on disk, so both the directory rule and the file rule follow `src/physics`.
        let moved = apply_renames(&mut m, &renames, |p| p != "src/core");
        assert_eq!(moved, 2);
        assert_eq!(m.nodes[dir_rule as usize].path, "src/physics");
        assert_eq!(m.nodes[file_rule as usize].path, "src/physics/collider.rs");
    }

    #[test]
    fn edited_method_inside_impl_is_the_symbol() {
        // Git's default header shows `impl Collider {` for an indented method: the rule anchored at
        // the edited method must not be penalised as "another symbol".
        let source = "impl Collider {\n    pub fn broad_phase(&self) {\n        let a = 1;\n    }\n\n    // the type of contact\n    pub fn resolve_contacts(&self) {\n        let z = 3;\n    }\n}\n";
        let diff = "diff --git a/src/core/collider.rs b/src/core/collider.rs\n--- a/src/core/collider.rs\n+++ b/src/core/collider.rs\n@@ -8 +8 @@ impl Collider {\n";
        let mut ts = rank_git_targets(" M src/core/collider.rs\n", diff, &[], &[], 12);
        assert_eq!(ts[0].symbols, vec!["Collider"], "header alone only knows the impl");
        refine_symbols(&mut ts, diff, |_| Some(source.to_string()));
        assert_eq!(ts[0].symbols, vec!["resolve_contacts", "Collider"]);
        assert_eq!(affinity("src/core/collider.rs", "resolve_contacts", &ts[0]), 1.0);
        assert_eq!(affinity("src/core/collider.rs", "broad_phase", &ts[0]), OTHER_SYMBOL);
        assert_eq!(affinity("src/core/collider.rs", "Collider", &ts[0]), 1.0, "type-level anchors still match");
        assert_eq!(enclosing_symbol(source, 3).as_deref(), Some("broad_phase"));
        assert_eq!(enclosing_symbol("// implements x\nlet a = 1;\n", 2), None, "comments are not declarations");
    }

    #[test]
    fn committed_renames_from_log_oldest_first() {
        let log = "R100\tsrc/physics/collider.rs\tsrc/sim/collider.rs\n\nR095\tsrc/core/collider.rs\tsrc/physics/collider.rs\n";
        assert_eq!(
            parse_log_renames(log),
            vec![
                ("src/core/collider.rs".to_string(), "src/physics/collider.rs".to_string()),
                ("src/physics/collider.rs".to_string(), "src/sim/collider.rs".to_string()),
            ]
        );
    }
}
