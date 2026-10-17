//! Repository verification (`cthg verify`).
//!
//! Checks memories against the live repository tree:
//! - Paths (`node.path` exists on disk).
//! - Symbols (`node.symbol` appears as an identifier inside `node.path`).
//! - Dependencies (`Cargo.toml` / `package.json`), only when the memory talks about dependencies
//!   or is anchored at a manifest.
//! - Commands (`NodeKind::Command` exits with status 0), only with `Options::run_commands`.
//!
//! State changes, and nothing else, move a memory (so running `verify` twice changes nothing):
//! - proven and anchorable (a fact, a command, or something from the repo) → `ANCHORED`;
//! - anchored but no longer holds → loses `ANCHORED` and its repo prior: the **only** way an
//!   anchored fact loses weight;
//! - check fails → `STALE` (E = −1 while set); passes again → `STALE` is cleared.
//!
//! Rules, decisions and conventions are never anchored: a symbol existing proves where a rule
//! applies, not that the rule is right.

use super::model::*;
use super::score::{self, View};
use super::spatial;
use super::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyAction {
    /// Already anchored, and the repository still proves it.
    Verified,
    /// Newly anchored because the repository proved it.
    Anchored,
    /// Was anchored, but the repository check failed: loses `ANCHORED` and drops in χ.
    Unanchored,
    /// Was not anchored, and its path, symbol, dependency or command no longer holds.
    Stale,
    /// Was `STALE`, and the repository matches it again.
    Restored,
}

impl VerifyAction {
    pub fn name(self) -> &'static str {
        match self {
            VerifyAction::Verified => "verificado",
            VerifyAction::Anchored => "anclado",
            VerifyAction::Unanchored => "pierde ancla",
            VerifyAction::Stale => "obsoleto",
            VerifyAction::Restored => "restaurado",
        }
    }
}

#[derive(Clone, Debug)]
pub struct VerifyEvent {
    pub node: u32,
    pub label: String,
    pub action: VerifyAction,
    pub reason: String,
    pub chi_before: f32,
    pub chi_after: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// false = dry run: report what would change, change nothing (used off the trunk).
    pub apply: bool,
    /// Run `Command` memories. Off by default: they come from Markdown and chat.
    pub run_commands: bool,
    /// Verify only these nodes (e.g. right after `learn --origin repo`).
    pub only: Option<Vec<u32>>,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub events: Vec<VerifyEvent>,
    /// Commands found but not run (no `run_commands`) or refused as unsafe.
    pub skipped_commands: Vec<(u32, String)>,
}

/// Callbacks that inspect the repository without coupling `cthg-core` to `std::process` or disk I/O.
pub struct RepoProbe<'a> {
    pub path_exists: &'a dyn Fn(&str) -> bool,
    pub read_file: &'a dyn Fn(&str) -> Option<String>,
    pub run_command: &'a dyn Fn(&str) -> Option<bool>,
}

enum CheckOutcome {
    /// Concrete proof found in the repo (can anchor a fact / command / repo node, or keep an anchor).
    Proven(String),
    /// Path exists, no finer check failed (keeps an anchor and lifts `STALE`, never anchors).
    PathOk(String),
    /// Check failed against the repo.
    Failed(String),
    /// Node makes no checkable claim against the repo.
    NotApplicable,
}

/// Prior a repo memory falls back to when its proof disappears: doubtful, little evidence.
const UNANCHORED_PRIOR: (f32, f32) = (1.5, 4.5);

/// Verifies every visible node (or `opts.only`) against `probe`.
pub fn verify(m: &mut Memory, view: &View, probe: &RepoProbe<'_>, opts: &Options) -> Report {
    let before = score::score_all(m, view);
    let mut work = m.clone();
    let mut report = Report::default();
    let mut raw: Vec<(u32, VerifyAction, String)> = Vec::new();

    for (n, b) in work.nodes.iter_mut().zip(before.iter()) {
        if !b.visible || opts.only.as_ref().is_some_and(|ids| !ids.contains(&n.id)) {
            continue;
        }
        let outcome = check_node(n, probe, opts, &mut report.skipped_commands);
        let was_anchored = n.anchored();
        let was_stale = n.has(flags::STALE);
        match outcome {
            CheckOutcome::Failed(reason) => {
                if was_anchored {
                    n.flags &= !(flags::ANCHORED | flags::CONSOLIDATED);
                    n.flags |= flags::STALE;
                    if n.origin == Origin::Repo {
                        (n.alpha, n.beta) = UNANCHORED_PRIOR;
                    }
                    raw.push((n.id, VerifyAction::Unanchored, reason));
                } else if !was_stale {
                    n.flags |= flags::STALE;
                    raw.push((n.id, VerifyAction::Stale, reason));
                }
            }
            CheckOutcome::Proven(reason) => {
                n.flags &= !flags::STALE;
                if was_anchored {
                    raw.push((n.id, VerifyAction::Verified, reason));
                } else if can_anchor(n) {
                    n.flags |= flags::ANCHORED;
                    raw.push((n.id, VerifyAction::Anchored, reason));
                } else if was_stale {
                    raw.push((n.id, VerifyAction::Restored, reason));
                }
            }
            CheckOutcome::PathOk(reason) => {
                n.flags &= !flags::STALE;
                if was_anchored {
                    raw.push((n.id, VerifyAction::Verified, reason));
                } else if was_stale {
                    raw.push((n.id, VerifyAction::Restored, reason));
                }
            }
            CheckOutcome::NotApplicable => {}
        }
    }

    let after = score::score_all(&work, view);
    report.events = raw
        .into_iter()
        .map(|(id, action, reason)| VerifyEvent {
            node: id,
            label: work.nodes[id as usize].label.clone(),
            action,
            reason,
            chi_before: before[id as usize].chi,
            chi_after: after[id as usize].chi,
        })
        .collect();
    if opts.apply {
        *m = work;
    }
    report
}

fn can_anchor(n: &Node) -> bool {
    n.origin == Origin::Repo || matches!(n.kind, NodeKind::Fact | NodeKind::Command)
}

fn check_node(n: &Node, probe: &RepoProbe<'_>, opts: &Options, skipped: &mut Vec<(u32, String)>) -> CheckOutcome {
    // 1. Check path existence if the node is spatially anchored.
    if !n.path.is_empty() && !(probe.path_exists)(&n.path) {
        return CheckOutcome::Failed(format!("ruta borrada: {}", n.path));
    }

    // 2. Check symbol existence inside the file if `n.symbol` is set.
    if !n.symbol.is_empty() && !n.path.is_empty() {
        return match (probe.read_file)(&n.path) {
            Some(content) if contains_ident(&content, &n.symbol) => {
                CheckOutcome::Proven(format!("símbolo '{}::{}' presente", n.path, n.symbol))
            }
            Some(_) => CheckOutcome::Failed(format!("símbolo '{}' ausente en {}", n.symbol, n.path)),
            // A directory or an unreadable file: the symbol cannot be checked, the path can.
            None => CheckOutcome::PathOk(format!("ruta existente: {}", n.path)),
        };
    }

    // 3. Check dependency claims (Cargo.toml / package.json).
    if let Some(dep_outcome) = check_dependency(n, probe) {
        return dep_outcome;
    }

    // 4. Check executable commands (`NodeKind::Command`), only when allowed and safe.
    if n.kind == NodeKind::Command {
        if let Some(cmd) = extract_command(&n.text) {
            if !opts.run_commands {
                skipped.push((n.id, cmd));
            } else if !is_safe_command(&cmd) {
                skipped.push((n.id, format!("{cmd} (rechazado: caracteres de shell)")));
            } else if let Some(ok) = (probe.run_command)(&cmd) {
                return if ok {
                    CheckOutcome::Proven(format!("comando válido: `{cmd}`"))
                } else {
                    CheckOutcome::Failed(format!("comando falló: `{cmd}`"))
                };
            }
        }
    }

    // 5. Only the path left: it proves a repo memory, and merely locates anything else.
    if n.path.is_empty() {
        CheckOutcome::NotApplicable
    } else if n.origin == Origin::Repo {
        CheckOutcome::Proven(format!("ruta verificada: {}", n.path))
    } else {
        CheckOutcome::PathOk(format!("ruta existente: {}", n.path))
    }
}

/// Words that make a memory a claim about dependencies.
const DEP_WORDS: &[&str] = &[
    "dependencia", "dependencias", "dependency", "dependencies", "crate", "crates", "paquete", "package",
];

fn check_dependency(n: &Node, probe: &RepoProbe<'_>) -> Option<CheckOutcome> {
    let folded = text::fold(&n.text);
    let talks_deps = folded.split_whitespace().any(|w| DEP_WORDS.contains(&w));
    let manifest_path = if n.path.ends_with("Cargo.toml") || n.path.ends_with("package.json") {
        n.path.clone()
    } else if !talks_deps {
        return None;
    } else if !n.path.is_empty() && (probe.path_exists)(&format!("{}/Cargo.toml", n.path)) {
        format!("{}/Cargo.toml", n.path)
    } else if (probe.path_exists)("Cargo.toml") {
        "Cargo.toml".to_string()
    } else if (probe.path_exists)("package.json") {
        "package.json".to_string()
    } else {
        return None;
    };

    let content = (probe.read_file)(&manifest_path)?;
    let deps = if manifest_path.ends_with("Cargo.toml") {
        parse_cargo_deps(&content)
    } else {
        parse_package_json_deps(&content)
    };

    // Claim of zero dependencies ("no tiene dependencias", "sin dependencias", "cero dependencias").
    if folded.contains("no tiene dependencia")
        || folded.contains("sin dependencia")
        || folded.contains("cero dependencia")
        || folded.contains("0 dependencia")
        || folded.contains("zero external dependenc")
        || folded.contains("no external dependenc")
        || folded.contains("zero dependenc")
    {
        return Some(if deps.is_empty() {
            CheckOutcome::Proven(format!("0 dependencias en {manifest_path}"))
        } else {
            CheckOutcome::Failed(format!("hay dependencias en {manifest_path} ({})", deps.join(", ")))
        });
    }

    // Anchored at a manifest without naming a dependency (a version, a feature…): not a dep claim.
    let candidate = extract_dep_name(&n.text, &folded)?;
    let norm_cand = normalize_dep(&candidate);
    if deps.iter().any(|d| normalize_dep(d) == norm_cand) {
        Some(CheckOutcome::Proven(format!("dependencia '{candidate}' presente en {manifest_path}")))
    } else {
        Some(CheckOutcome::Failed(format!("dependencia '{candidate}' ausente en {manifest_path}")))
    }
}

/// No pipes, redirections, chaining, expansions or quotes: one program with plain arguments.
pub fn is_safe_command(cmd: &str) -> bool {
    !cmd.is_empty() && !cmd.chars().any(|c| "&|;<>$`%^!()\n\r\"'".contains(c))
}

fn normalize_dep(s: &str) -> String {
    s.trim().to_lowercase().replace('-', "_")
}

fn extract_dep_name(raw: &str, folded: &str) -> Option<String> {
    // 1. Backticked single identifier (excluding manifest filenames and common keywords).
    let mut parts = raw.split('`');
    let _ = parts.next();
    while let Some(span) = parts.next() {
        let s = span.trim();
        if !s.is_empty()
            && !s.contains(char::is_whitespace)
            && !s.contains('/')
            && !s.ends_with(".toml")
            && !s.ends_with(".json")
            && !s.ends_with(".rs")
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Some(s.to_string());
        }
        let _ = parts.next();
    }

    // 2. Token immediately following "usa ", "dependencia ", "crate ", "paquete ".
    let words: Vec<&str> = folded.split_whitespace().collect();
    for (i, &w) in words.iter().enumerate() {
        if matches!(w, "usa" | "dependencia" | "crate" | "paquete") {
            if let Some(&next) = words.get(i + 1) {
                if !matches!(next, "el" | "la" | "un" | "una" | "como" | "para" | "en" | "de" | "sin" | "ninguna" | "externa" | "externas") {
                    return Some(next.to_string());
                }
            }
        }
    }
    None
}

/// Parses dependency keys from `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]` and `[workspace.dependencies]`.
pub fn parse_cargo_deps(toml: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(sec) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let sec = sec.trim();
            in_deps = sec == "dependencies"
                || sec == "dev-dependencies"
                || sec == "build-dependencies"
                || sec == "workspace.dependencies"
                || sec.ends_with(".dependencies");
            continue;
        }
        if in_deps {
            if let Some((k, _)) = trimmed.split_once('=') {
                let name = k.trim().trim_matches('"');
                if !name.is_empty() && !deps.iter().any(|d| d == name) {
                    deps.push(name.to_string());
                }
            }
        }
    }
    deps
}

fn parse_package_json_deps(json: &str) -> Vec<String> {
    let mut deps = Vec::new();
    for key in ["\"dependencies\"", "\"devDependencies\""] {
        let Some(pos) = json.find(key) else { continue };
        let after = &json[pos + key.len()..];
        let Some(open) = after.find('{') else { continue };
        let Some(close) = after[open + 1..].find('}') else { continue };
        let block = &after[open + 1..open + 1 + close];
        for line in block.lines() {
            if let Some((k, _)) = line.trim().split_once(':') {
                let name = k.trim().trim_matches('"');
                if !name.is_empty() && !deps.iter().any(|d| d == name) {
                    deps.push(name.to_string());
                }
            }
        }
    }
    deps
}

pub fn contains_ident(source: &str, ident: &str) -> bool {
    if ident.is_empty() {
        return false;
    }
    source
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|tok| tok == ident)
}

pub fn extract_command(raw: &str) -> Option<String> {
    let mut parts = raw.split('`');
    let _ = parts.next();
    while let Some(span) = parts.next() {
        let s = span.trim();
        if is_runnable_cmd(s) {
            return Some(s.to_string());
        }
        let _ = parts.next();
    }
    let trimmed = raw
        .trim()
        .strip_prefix("Ejecuta ")
        .or_else(|| raw.trim().strip_prefix("ejecuta "))
        .or_else(|| raw.trim().strip_prefix("Comando: "))
        .or_else(|| raw.trim().strip_prefix("comando: "))
        .unwrap_or(raw.trim())
        .trim_end_matches('.');
    is_runnable_cmd(trimmed).then(|| trimmed.to_string())
}

fn is_runnable_cmd(s: &str) -> bool {
    let first = s.split_whitespace().next().unwrap_or("");
    matches!(
        first,
        "cargo" | "git" | "npm" | "pnpm" | "yarn" | "pytest" | "python" | "node" | "go" | "make" | "cthg" | "rustc"
    ) && !spatial::normalize(s).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::{learn, Claim};
    use crate::score::{score_all, View};

    fn fact(m: &mut Memory, text: &str, path: &str, origin: Origin, y: f64) -> u32 {
        let mut c = Claim::rule(text, path);
        c.kind = NodeKind::Fact;
        c.origin = origin;
        learn(m, &c, y)[0].node
    }

    fn apply() -> Options {
        Options { apply: true, run_commands: true, only: None }
    }

    #[test]
    fn verify_unanchors_broken_repo_facts_and_anchors_proven_ones() {
        let mut m = Memory::default();
        let tokio_id = fact(&mut m, "El proyecto usa tokio como runtime asíncrono", "Cargo.toml", Origin::Repo, 100.0);
        assert!(m.nodes[tokio_id as usize].anchored());

        let mut sym_fact = Claim::rule("La función resolve_contacts resuelve penetración", "src/core/collider.rs");
        sym_fact.kind = NodeKind::Fact;
        sym_fact.symbol = "resolve_contacts".into();
        let sym_id = learn(&mut m, &sym_fact, 101.0)[0].node;

        let old_id = learn(&mut m, &Claim::rule("No toques este módulo antiguo", "src/legacy/old.rs"), 102.0)[0].node;

        let mut cmd_claim = Claim::rule("Ejecuta `cargo test` para validar el motor", "");
        cmd_claim.kind = NodeKind::Command;
        let cmd_id = learn(&mut m, &cmd_claim, 103.0)[0].node;

        let probe = RepoProbe {
            path_exists: &|p| matches!(p, "Cargo.toml" | "src/core/collider.rs"),
            read_file: &|p| match p {
                "Cargo.toml" => Some("[package]\nname = \"demo\"\n\n[dependencies]\n".into()),
                "src/core/collider.rs" => Some("pub fn resolve_contacts() {}\n".into()),
                _ => None,
            },
            run_command: &|cmd| Some(cmd == "cargo test"),
        };

        let report = verify(&mut m, &View::all(), &probe, &apply());
        let scores = score_all(&m, &View::all());
        let ev = |id: u32| report.events.iter().find(|e| e.node == id).unwrap();

        assert_eq!(ev(tokio_id).action, VerifyAction::Unanchored);
        assert!(!m.nodes[tokio_id as usize].anchored());
        assert!(scores[tokio_id as usize].chi < 0.0, "unanchored repo fact lost its weight");

        assert_eq!(ev(sym_id).action, VerifyAction::Anchored);
        assert!(scores[sym_id as usize].chi >= 0.9);

        assert_eq!(ev(old_id).action, VerifyAction::Stale);
        assert!(ev(old_id).chi_after < ev(old_id).chi_before);

        assert_eq!(ev(cmd_id).action, VerifyAction::Anchored);
    }

    #[test]
    fn verify_is_idempotent_and_reversible() {
        let mut m = Memory::default();
        let rule = learn(&mut m, &Claim::rule("No uses unwrap en el colisionador", "src/core/collider.rs"), 100.0)[0].node;
        let (a0, b0) = (m.nodes[rule as usize].alpha, m.nodes[rule as usize].beta);
        let chi0 = score_all(&m, &View::all())[rule as usize].chi;

        let gone = RepoProbe { path_exists: &|_| false, read_file: &|_| None, run_command: &|_| None };
        assert_eq!(verify(&mut m, &View::all(), &gone, &apply()).events[0].action, VerifyAction::Stale);
        assert!(score_all(&m, &View::all())[rule as usize].chi < chi0);
        assert!(verify(&mut m, &View::all(), &gone, &apply()).events.is_empty(), "second run changes nothing");
        assert_eq!((m.nodes[rule as usize].alpha, m.nodes[rule as usize].beta), (a0, b0), "evidence untouched");

        let back = RepoProbe { path_exists: &|_| true, read_file: &|_| None, run_command: &|_| None };
        assert_eq!(verify(&mut m, &View::all(), &back, &apply()).events[0].action, VerifyAction::Restored);
        assert!((score_all(&m, &View::all())[rule as usize].chi - chi0).abs() < 1e-6);
    }

    #[test]
    fn rules_are_not_anchored_and_unrelated_facts_are_not_dependency_checked() {
        let mut m = Memory::default();
        let mut rule = Claim::rule("En resolve_contacts los contactos se ordenan por id", "src/core/collider.rs");
        rule.symbol = "resolve_contacts".into();
        let rule = learn(&mut m, &rule, 100.0)[0].node;
        let f64_fact = fact(&mut m, "El motor usa f64 para las posiciones", "src/core", Origin::User, 101.0);
        let version = fact(&mut m, "La versión publicada es la 0.1", "Cargo.toml", Origin::User, 102.0);

        let probe = RepoProbe {
            path_exists: &|_| true,
            read_file: &|p| match p {
                "src/core/collider.rs" => Some("fn resolve_contacts() {}".into()),
                "Cargo.toml" => Some("[package]\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n".into()),
                _ => None,
            },
            run_command: &|_| None,
        };
        let report = verify(&mut m, &View::all(), &probe, &apply());
        assert!(!m.nodes[rule as usize].anchored(), "a symbol existing does not prove a rule");
        assert!(report.events.iter().all(|e| e.node != f64_fact), "`usa f64` is not a dependency claim");
        assert!(!m.nodes[f64_fact as usize].has(flags::STALE));
        assert!(!m.nodes[version as usize].anchored(), "Cargo.toml existing does not prove the version");
        assert!(!m.nodes[version as usize].has(flags::STALE));
    }

    #[test]
    fn dry_run_and_commands_need_permission() {
        let mut m = Memory::default();
        let mut chained = Claim::rule("Ejecuta `cargo test && git push` antes de terminar", "");
        chained.kind = NodeKind::Command;
        learn(&mut m, &chained, 100.0);
        let mut plain = Claim::rule("Ejecuta `cargo build` para compilar", "");
        plain.kind = NodeKind::Command;
        let plain = learn(&mut m, &plain, 101.0)[0].node;
        let ran = std::cell::Cell::new(0);
        let run = |_: &str| {
            ran.set(ran.get() + 1);
            Some(true)
        };
        let probe = RepoProbe { path_exists: &|_| true, read_file: &|_| None, run_command: &run };

        let r = verify(&mut m, &View::all(), &probe, &Options::default());
        assert_eq!((ran.get(), r.skipped_commands.len()), (0, 2), "nothing runs without permission");

        let r = verify(&mut m, &View::all(), &probe, &Options { apply: false, run_commands: true, only: None });
        assert_eq!(ran.get(), 1, "the chained command is refused, the plain one runs");
        assert_eq!(r.events[0].action, VerifyAction::Anchored);
        assert!(!m.nodes[plain as usize].anchored(), "dry run changes nothing");
    }
}
