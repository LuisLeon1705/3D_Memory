//! Markdown importer (`cthg import <archivo.md>`).
//!
//! Splits a `CLAUDE.md` or `AGENTS.md` into memory nodes with origin `Imported`:
//! - One bullet or paragraph per node (fenced code blocks, headings and tables are skipped).
//! - Deduces `path` (and optional `::symbol`) only when the line names a path that actually
//!   exists in the repository; everything else stays at the root (`path = ""`).
//! - Proposes `CORE` for pathless nodes that express general prohibitions or obligations,
//!   without setting `flags::CORE` until confirmed (`--yes` or `cthg core <id>`).
//! - Idempotent: importing the same file twice does not duplicate nodes or inflate `alpha`.

use super::learn;
use super::model::*;
use super::score::{self, View};
use super::spatial;
use super::text;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportItem {
    pub text: String,
    pub kind: NodeKind,
    pub path: String,
    pub symbol: String,
    pub propose_core: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    /// Newly created node IDs.
    pub added: Vec<u32>,
    /// Existing node IDs that matched an imported item (not duplicated).
    pub existing: Vec<u32>,
    /// Pathless general rules proposed for `CORE` (when `apply_core` is false).
    pub proposed_core: Vec<u32>,
    /// Node IDs marked as `CORE` during this import (when `apply_core` is true).
    pub marked_core: Vec<u32>,
}

const NORMATIVE_WORDS: &[&str] = &[
    // es — prohibitions & obligations
    "nunca", "jamas", "prohibido", "prohibida", "evita", "evitar", "siempre", "obligatorio",
    "obligatoria", "debe", "deben", "debes", "asegurate", "manten", "mantener", "respeta",
    "respetar", "conserva", "preserva", "imita", "comprueba", "verifica", "unicamente",
    // en — prohibitions & obligations
    "never", "avoid", "forbidden", "dont", "always", "must", "required", "mandatory", "ensure",
    "preserve", "prefer",
];

const NORMATIVE_PHRASES: &[&str] = &[
    "no uses",
    "no hagas",
    "no borres",
    "no cambies",
    "no modifiques",
    "no anadas",
    "no toques",
    "no reescribas",
    "no empieces",
    "no tiene",
    "no puede",
    "no pueden",
    "no se permite",
    "tiene que",
    "tienen que",
    "hay que",
    "antes de",
    "al terminar",
    "en ingles",
    "en espanol",
    "sin dependencias",
    "sin unwrap",
    "do not",
    "don t",
    "must not",
    "make sure",
];

/// Parses a Markdown document into candidate memory items.
pub fn parse_markdown(md: &str, exists: impl Fn(&str) -> bool) -> Vec<ImportItem> {
    let raw_blocks = split_blocks(md);
    let mut out: Vec<ImportItem> = Vec::new();
    for (raw, is_bullet) in raw_blocks {
        let (path, symbol) = extract_location(&raw, &exists);
        let cleaned = clean_markdown(&raw);
        let toks = text::tokens(&cleaned);
        if toks.len() < 2 {
            continue;
        }
        // Skip short section lead-ins like "Al terminar una fase:" or "Reglas del código:".
        if cleaned.ends_with(':') && toks.len() <= 6 && path.is_empty() {
            continue;
        }
        let kind = classify_kind(&raw, &cleaned, is_bullet);
        let propose_core = is_core_candidate(&cleaned, &path, kind);
        if out
            .iter()
            .any(|prev| prev.path == path && text::similarity(&toks, &text::tokens(&prev.text)) >= 0.85)
        {
            continue;
        }
        out.push(ImportItem { text: cleaned, kind, path, symbol, propose_core });
    }
    out
}

/// Imports a Markdown document into `m`. When `apply_core` is false, `CORE` candidates are only
/// reported in `proposed_core`; when true, they receive `flags::CORE`.
pub fn import_markdown(
    m: &mut Memory,
    md: &str,
    branch: &str,
    commit: &str,
    now: f64,
    apply_core: bool,
    exists: impl Fn(&str) -> bool,
) -> ImportReport {
    let items = parse_markdown(md, exists);
    let view = View::on_branch(f64::INFINITY, branch);
    let scores = score::score_all(m, &view);
    let mut report = ImportReport::default();

    for item in items {
        let toks = text::tokens(&item.text);
        if let Some(id) = find_existing(m, &scores, &toks, &item.path) {
            if !report.added.contains(&id) && !report.existing.contains(&id) {
                report.existing.push(id);
            }
            if m.nodes[id as usize].path.is_empty() && !item.path.is_empty() {
                m.nodes[id as usize].path = item.path.clone();
                m.nodes[id as usize].symbol = item.symbol.clone();
            }
            let is_pathless = m.nodes[id as usize].path.is_empty();
            if item.propose_core && is_pathless && !m.nodes[id as usize].has(flags::CORE) {
                if apply_core {
                    m.nodes[id as usize].flags |= flags::CORE;
                    if !report.marked_core.contains(&id) {
                        report.marked_core.push(id);
                    }
                } else if !report.proposed_core.contains(&id) {
                    report.proposed_core.push(id);
                }
            }
            continue;
        }

        let y = learn::next_y(m, now);
        let (alpha, beta) = Origin::Imported.prior();
        let mut node_flags = 0u8;
        if item.kind == NodeKind::Plan {
            node_flags |= flags::POTENTIAL;
        } else if item.propose_core && apply_core {
            node_flags |= flags::CORE;
        }
        let id = m.add_node(Node {
            id: 0,
            kind: item.kind,
            origin: Origin::Imported,
            flags: node_flags,
            path: item.path,
            symbol: item.symbol,
            branch: branch.to_string(),
            commit: commit.to_string(),
            y,
            alpha,
            beta,
            activation: 1.0,
            recalls: 0,
            created_session: m.session,
            key: String::new(),
            label: text::clip(&item.text, 64),
            text: text::clip(&item.text, 600),
            aliases: vec![],
        });
        report.added.push(id);
        if item.propose_core && item.kind != NodeKind::Plan {
            if apply_core {
                report.marked_core.push(id);
            } else {
                report.proposed_core.push(id);
            }
        }
    }
    report
}

fn find_existing(m: &Memory, scores: &[score::Score], toks: &[String], path: &str) -> Option<u32> {
    m.nodes
        .iter()
        .filter(|n| {
            scores
                .get(n.id as usize)
                .map(|s| s.visible || n.has(flags::POTENTIAL))
                .unwrap_or(true)
        })
        .filter(|n| n.kind.is_statement() && paths_compatible(&n.path, path))
        .map(|n| {
            let ntoks = text::tokens(&format!("{} {}", n.label, n.text));
            (n.id, text::similarity(toks, &ntoks))
        })
        .filter(|&(_, sim)| sim >= 0.65)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(id, _)| id)
}

fn paths_compatible(a: &str, b: &str) -> bool {
    spatial::is_ancestor_or_self(a, b) || spatial::is_ancestor_or_self(b, a)
}

/// Splits Markdown into `(raw_block, is_bullet)` items, skipping code fences, headings and tables.
fn split_blocks(md: &str) -> Vec<(String, bool)> {
    let md = md.strip_prefix('\u{feff}').unwrap_or(md);
    let mut blocks = Vec::new();
    let mut cur = String::new();
    let mut cur_bullet = false;
    let mut in_fence = false;

    let flush = |blocks: &mut Vec<(String, bool)>, cur: &mut String, cur_bullet: &mut bool| {
        let s = cur.trim();
        if !s.is_empty() {
            blocks.push((s.to_string(), *cur_bullet));
        }
        cur.clear();
        *cur_bullet = false;
    };

    for line in md.lines() {
        let trimmed_start = line.trim_start_matches([' ', '\t', '\u{feff}']);
        if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
            flush(&mut blocks, &mut cur, &mut cur_bullet);
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let trimmed = trimmed_start.trim_end();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with('|')
            || is_horizontal_rule(trimmed)
            || is_html_tag(trimmed)
            || trimmed.starts_with("> [!")
        {
            flush(&mut blocks, &mut cur, &mut cur_bullet);
            continue;
        }
        let content = trimmed.strip_prefix("> ").unwrap_or(trimmed).trim_start();
        if let Some(item) = strip_bullet(content) {
            flush(&mut blocks, &mut cur, &mut cur_bullet);
            cur.push_str(item);
            cur_bullet = true;
        } else if !cur.is_empty() {
            cur.push(' ');
            cur.push_str(content);
        } else {
            cur.push_str(content);
            cur_bullet = false;
        }
    }
    flush(&mut blocks, &mut cur, &mut cur_bullet);
    blocks
}

fn is_horizontal_rule(s: &str) -> bool {
    s.len() >= 3
        && (s.chars().all(|c| c == '-' || c == ' ')
            || s.chars().all(|c| c == '*' || c == ' ')
            || s.chars().all(|c| c == '_' || c == ' '))
        && s.chars().filter(|&c| c != ' ').count() >= 3
}

fn is_html_tag(s: &str) -> bool {
    s.starts_with('<') && s.ends_with('>')
}

fn strip_bullet(s: &str) -> Option<&str> {
    for prefix in ["- ", "* ", "+ ", "• "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return Some(rest.trim_start());
        }
    }
    let digits = s.bytes().take_while(|b| b.is_ascii_digit()).count();
    if (1..=3).contains(&digits) {
        let rest = &s[digits..];
        if let Some(after) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(after.trim_start());
        }
    }
    None
}

/// Cleans Markdown links `[label](url)` and emphasis markers `**` / `__`.
fn clean_markdown(s: &str) -> String {
    let s = s
        .strip_prefix("[ ] ")
        .or_else(|| s.strip_prefix("[x] "))
        .or_else(|| s.strip_prefix("[X] "))
        .unwrap_or(s);
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(close_br) = s[i + 1..].find("](") {
                let label_end = i + 1 + close_br;
                let url_start = label_end + 2;
                if let Some(close_par) = s[url_start..].find(')') {
                    let label = &s[i + 1..label_end];
                    out.push_str(label);
                    i = url_start + close_par + 1;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    let out = out.replace("**", "").replace("__", "");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Finds the most specific repository path (and optional `::symbol`) mentioned in `raw`.
fn extract_location(raw: &str, exists: &impl Fn(&str) -> bool) -> (String, String) {
    let mut candidates: Vec<(String, bool)> = Vec::new();

    // 1. Markdown link targets `[label](target)`
    let mut rest = raw;
    while let Some(idx) = rest.find("](") {
        let after = &rest[idx + 2..];
        if let Some(end) = after.find(')') {
            candidates.push((after[..end].trim().to_string(), true));
            rest = &after[end + 1..];
        } else {
            break;
        }
    }

    // 2. Backtick spans `` `code` ``
    let mut parts = raw.split('`');
    let _ = parts.next();
    while let Some(code) = parts.next() {
        let code = code.trim();
        if !code.is_empty() && !code.contains(char::is_whitespace) {
            candidates.push((code.to_string(), true));
        } else {
            for w in code.split_whitespace() {
                candidates.push((w.to_string(), false));
            }
        }
        let _ = parts.next();
    }

    // 3. Plain tokens in the text
    for w in raw.split_whitespace() {
        candidates.push((w.to_string(), false));
    }

    let mut best: Option<(String, String)> = None;
    for (raw_cand, explicit) in candidates {
        let Some((path, sym)) = parse_candidate(&raw_cand, explicit, exists) else {
            continue;
        };
        let replace = match &best {
            None => true,
            Some((bp, bsym)) => {
                (spatial::depth(&path), !sym.is_empty(), path.len())
                    > (spatial::depth(bp), !bsym.is_empty(), bp.len())
            }
        };
        if replace {
            best = Some((path, sym));
        }
    }
    best.unwrap_or_default()
}

fn parse_candidate(raw: &str, explicit: bool, exists: &impl Fn(&str) -> bool) -> Option<(String, String)> {
    let trimmed = raw.trim_matches(|c: char| {
        matches!(
            c,
            '`' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';' | '"' | '\'' | '«' | '»' | '¿' | '?' | '¡' | '!'
        )
    });
    if trimmed.is_empty()
        || trimmed.contains("://")
        || trimmed.starts_with("mailto:")
        || trimmed.starts_with('#')
    {
        return None;
    }
    let without_frag = trimmed.split('#').next().unwrap_or(trimmed);
    let (path_part, sym_part) = match without_frag.split_once("::") {
        Some((p, s)) => (p, s.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_')),
        None => (without_frag, ""),
    };
    let clean_path = path_part.trim_end_matches(['.', ':', ',', ';']);
    if !explicit && !looks_like_path(clean_path) {
        return None;
    }
    let norm = spatial::normalize(clean_path);
    if !is_valid_repo_path(&norm) || !exists(&norm) {
        return None;
    }
    let sym = if !sym_part.is_empty() && sym_part.chars().all(|c| c.is_alphanumeric() || c == '_') {
        sym_part.to_string()
    } else {
        String::new()
    };
    Some((norm, sym))
}

fn looks_like_path(s: &str) -> bool {
    if s.contains('/') || s.contains('\\') {
        return true;
    }
    if let Some((stem, ext)) = s.rsplit_once('.') {
        !stem.is_empty()
            && (2..=6).contains(&ext.len())
            && ext.chars().all(|c| c.is_ascii_alphabetic())
            && stem.chars().any(|c| c.is_ascii_alphabetic())
    } else {
        false
    }
}

fn is_valid_repo_path(p: &str) -> bool {
    !p.is_empty()
        && p != "."
        && p != ".."
        && !p.starts_with("../")
        && !p.contains("/../")
        && p != ".git"
        && !p.starts_with(".git/")
        && p != ".cthg"
        && !p.starts_with(".cthg/")
}

fn classify_kind(raw: &str, cleaned: &str, is_bullet: bool) -> NodeKind {
    let folded = text::fold(cleaned);
    if raw.starts_with("[ ] ")
        || cleaned.starts_with("TODO:")
        || cleaned.starts_with("TODO ")
        || folded.starts_with("pendiente ")
        || folded.starts_with("por hacer ")
    {
        return NodeKind::Plan;
    }
    if !is_prohibition(&folded)
        && raw.contains('`')
        && (folded.starts_with("ejecuta ")
            || folded.starts_with("comando ")
            || folded.starts_with("corre ")
            || folded.contains("cargo ")
            || folded.contains("npm ")
            || folded.contains("pytest "))
        && !is_obligation(&folded)
    {
        return NodeKind::Command;
    }
    if folded.contains("convencion")
        || folded.contains("nomenclatura")
        || folded.contains("identificadores en ")
        || folded.contains("comentarios en ")
    {
        return NodeKind::Convention;
    }
    if is_normative(&folded) || is_bullet {
        NodeKind::Rule
    } else {
        NodeKind::Fact
    }
}

/// True when a pathless item expresses a general prohibition or obligation.
pub fn is_core_candidate(cleaned: &str, path: &str, kind: NodeKind) -> bool {
    if !path.is_empty() || kind == NodeKind::Plan {
        return false;
    }
    let folded = text::fold(cleaned);
    is_normative(&folded)
}

fn is_prohibition(folded: &str) -> bool {
    folded.starts_with("no ")
        || folded.starts_with("sin ")
        || folded.starts_with("nunca ")
        || folded.starts_with("jamas ")
        || folded.starts_with("never ")
        || folded
            .split_whitespace()
            .any(|w| matches!(w, "nunca" | "jamas" | "prohibido" | "prohibida" | "never" | "forbidden"))
}

fn is_obligation(folded: &str) -> bool {
    folded.split_whitespace().any(|w| {
        matches!(
            w,
            "siempre" | "obligatorio" | "obligatoria" | "debe" | "deben" | "debes" | "always" | "must" | "mandatory"
        )
    })
}

fn is_normative(folded: &str) -> bool {
    if folded.starts_with("no ") || folded.starts_with("sin ") {
        return true;
    }
    if folded.split_whitespace().any(|w| NORMATIVE_WORDS.contains(&w)) {
        return true;
    }
    NORMATIVE_PHRASES.iter().any(|p| text::contains_phrase(folded, p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::{score_all, View};

    const SAMPLE_MD: &str = "\u{feff}\
# Guía de trabajo

Este repositorio implementa el motor físico del juego.

```bash
cargo test
```

## Reglas

- Nunca hagas push directo a main ni reescribas la historia.
- Código, identificadores y comentarios siempre en inglés.
- En `src/core/collider.rs::resolve_contacts` no uses `unwrap()` porque puede tumbar el servidor.
- Todo lo que cuelga de `src/core/` debe ser determinista, sin `rand`.
- La ruta `src/fantasma/inexistente.rs` aún no existe en el árbol.
- [ ] Pendiente: migrar el renderizador a wgpu.
";

    fn repo_exists(p: &str) -> bool {
        matches!(p, "src" | "src/core" | "src/core/collider.rs" | "Cargo.toml")
    }

    #[test]
    fn splits_markdown_deduces_existing_paths_and_proposes_core() {
        let items = parse_markdown(SAMPLE_MD, repo_exists);
        assert_eq!(items.len(), 7);

        // Descriptive intro paragraph without path or obligation is not proposed as CORE.
        assert_eq!(items[0].path, "");
        assert!(!items[0].propose_core);

        // Pathless prohibition and obligation are proposed as CORE.
        assert!(items[1].text.contains("push directo a main"));
        assert_eq!(items[1].path, "");
        assert!(items[1].propose_core);

        assert!(items[2].text.contains("comentarios siempre en inglés"));
        assert_eq!(items[2].path, "");
        assert!(items[2].propose_core);

        // Existing file + symbol deduced; because it has a path, it is NOT proposed as CORE.
        assert_eq!(items[3].path, "src/core/collider.rs");
        assert_eq!(items[3].symbol, "resolve_contacts");
        assert!(!items[3].propose_core);

        // Existing directory deduced (`src/core/` -> `src/core`), and Spanish "Todo" is Rule, not Plan.
        assert_eq!(items[4].path, "src/core");
        assert_eq!(items[4].kind, NodeKind::Rule);
        assert!(!items[4].propose_core);

        // Non-existent path stays empty (`path = ""`).
        assert_eq!(items[5].path, "");

        // Task item `[ ]` becomes a Plan (`POTENTIAL`), never proposed as CORE.
        assert_eq!(items[6].kind, NodeKind::Plan);
        assert!(!items[6].propose_core);
    }

    #[test]
    fn importing_twice_is_idempotent_and_yes_confirms_core() {
        let mut m = Memory::default();

        // First import without confirmation: nodes are created with Origin::Imported, none is CORE yet.
        let r1 = import_markdown(&mut m, SAMPLE_MD, "", "a1b2c3d", 1000.0, false, repo_exists);
        assert_eq!(r1.added.len(), 7);
        assert!(r1.existing.is_empty());
        assert_eq!(r1.proposed_core.len(), 2);
        assert!(r1.marked_core.is_empty());
        assert!(m.nodes.iter().all(|n| n.origin == Origin::Imported));
        assert!(m.nodes.iter().all(|n| !n.has(flags::CORE)));

        let scores_before = score_all(&m, &View::all());

        // Second import without `--yes`: zero duplicates, χ does not inflate, proposals remain.
        let r2 = import_markdown(&mut m, SAMPLE_MD, "", "a1b2c3d", 1005.0, false, repo_exists);
        assert!(r2.added.is_empty(), "must not duplicate nodes");
        assert_eq!(r2.existing.len(), 7);
        assert_eq!(r2.proposed_core, r1.proposed_core);
        assert_eq!(m.nodes.len(), 7);
        let scores_after = score_all(&m, &View::all());
        for i in 0..m.nodes.len() {
            assert!((scores_before[i].chi - scores_after[i].chi).abs() < 1e-6);
        }

        // Confirming with `apply_core = true` (`--yes`) marks only the proposed nodes as CORE.
        let r3 = import_markdown(&mut m, SAMPLE_MD, "", "a1b2c3d", 1010.0, true, repo_exists);
        assert!(r3.added.is_empty());
        assert_eq!(r3.marked_core, r1.proposed_core);
        for &id in &r3.marked_core {
            assert!(m.nodes[id as usize].has(flags::CORE));
            assert!(m.nodes[id as usize].path.is_empty());
        }
    }
}
