//! Markdown importer (`cthg import <archivo.md>`).
//!
//! Splits a `CLAUDE.md` or `AGENTS.md` into memory nodes with origin `Imported`:
//! - One bullet or paragraph per node (fenced code blocks, headings and tables are skipped).
//! - Deduces `path` (and optional `::symbol`) only when the line names a path that actually
//!   exists in the repository; everything else stays at the root (`path = ""`). A bare file
//!   name (`binary.rs`) counts when it names exactly one file of the repo. Mentioning a
//!   document (`GUIA.md`, `docs/…`) does not anchor a rule to it, unless the block is about that
//!   document (`En README.md …`): "update the guide" is not "this rule lives in the guide".
//! - Keeps where each block sits: its `##` headings and the lead-in that introduces it
//!   ("Al terminar una fase:") become the node's context, so "pasa las dos comprobaciones"
//!   still means something on its own.
//! - Proposes `CORE` as a ranked selection that fits the session-start budget: pathless rules
//!   scored by how binding they are, whether the author emphasised them, and whether their
//!   section is general or about one command/module (those are found lexically instead).
//!   Nothing becomes `CORE` until confirmed (`--yes` or `cthg core <id>`).
//! - Idempotent: importing the same file twice does not duplicate nodes or inflate `alpha`.

use super::learn;
use super::model::*;
use super::score::{self, View};
use super::spatial;
use super::text;

#[derive(Clone, Debug, PartialEq)]
pub struct ImportItem {
    /// 1-indexed line number of the first line of the Markdown block.
    pub line: usize,
    pub text: String,
    /// Headings (level ≥ 2) and lead-ins above the block, joined with ` › `. Empty at top level.
    pub context: String,
    pub kind: NodeKind,
    pub path: String,
    pub symbol: String,
    /// Written in **bold** (or `__…__`) by the author.
    pub emphasis: bool,
    /// How much it deserves to be injected every session (`core_score`); `None` = not a candidate.
    pub core_score: Option<f32>,
    /// Selected for `CORE` within the budget (`select_core`).
    pub propose_core: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    /// Newly created node IDs.
    pub added: Vec<u32>,
    /// Existing node IDs that matched an imported item (not duplicated).
    pub existing: Vec<u32>,
    /// `(line, merged_into_node_id)` for items that `find_existing` merged into another node.
    pub merged: Vec<(usize, u32)>,
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

/// Parses a Markdown document into candidate memory items. `resolve` maps a mentioned path (or a
/// bare file name) to the repository path it names, or `None` if there is none.
pub fn parse_markdown(md: &str, resolve: impl Fn(&str) -> Option<String>) -> Vec<ImportItem> {
    let mut out: Vec<ImportItem> = Vec::new();
    // Lead-ins ("Al terminar una fase:") apply to the bullets indented below them.
    // A paragraph lead-in (indent −1) applies to every bullet until the next paragraph.
    let mut lead_ins: Vec<(i64, String)> = Vec::new();
    let mut last_headings: Vec<String> = Vec::new();
    for b in split_blocks(md) {
        if b.headings != last_headings {
            lead_ins.clear();
            last_headings = b.headings.clone();
        }
        let indent = if b.bullet { b.indent as i64 } else { -1 };
        if b.bullet {
            lead_ins.retain(|(i, _)| *i < indent);
        } else {
            lead_ins.clear();
        }
        let cleaned = clean_markdown(&b.raw);
        let context: Vec<String> = b
            .headings
            .iter()
            .cloned()
            .chain(lead_ins.iter().map(|(_, t)| t.trim_end_matches(':').trim().to_string()))
            .collect();
        if cleaned.ends_with(':') {
            lead_ins.push((indent, cleaned.clone()));
        }
        let (path, symbol) = extract_location(&b.raw, &cleaned, &resolve);
        let toks = text::tokens(&cleaned);
        if toks.len() < 2 {
            continue;
        }
        // Skip short section lead-ins like "Al terminar una fase:": they live on as context.
        if cleaned.ends_with(':') && toks.len() <= 6 && path.is_empty() {
            continue;
        }
        let kind = classify_kind(&b.raw, &cleaned, b.bullet);
        let mut item = ImportItem {
            line: b.line,
            text: cleaned,
            context: context.join(" › "),
            kind,
            path,
            symbol,
            emphasis: b.raw.contains("**") || b.raw.contains("__"),
            core_score: None,
            propose_core: false,
        };
        item.core_score = core_score(&item);
        out.push(item);
    }
    select_core(&mut out, BUDGET_CORE);
    out
}

/// Session-start budget (tokens) the `CORE` proposal must fit in.
pub const BUDGET_CORE: usize = super::hook::BUDGET_CORE;
/// Room for the `## Reglas del proyecto (siempre)` heading and the band line.
const CORE_HEADER_TOKENS: usize = 12;
/// Below this score a rule is not worth a line in every session.
const CORE_MIN_SCORE: f32 = 1.0;

/// How much a rule deserves to be injected in every session, or `None` if it is not a candidate:
///
/// score = binding (prohibition 2 · obligation 1,5 · other normative 1)
///       + 1   if the author emphasised it
///
/// Never candidates: rules with a path (the spatial channel finds them), rules in a section
/// about one command or module (a heading with `code`: the lexical channel finds them when the
/// task names it), plans (not facts) and plain descriptions (they bind nothing).
pub fn core_score(item: &ImportItem) -> Option<f32> {
    if !item.path.is_empty() || item.kind == NodeKind::Plan || item.context.contains('`') {
        return None;
    }
    let folded = text::fold(&item.text);
    let binding = if is_prohibition(&folded) {
        2.0
    } else if is_obligation(&folded) {
        1.5
    } else if is_normative(&folded) {
        1.0
    } else {
        return None;
    };
    let mut score = binding;
    if item.emphasis {
        score += 1.0;
    }
    Some(score)
}

/// Approximate tokens of the line the rule takes when injected (`- #NN regla: …`).
fn core_cost(item: &ImportItem) -> usize {
    text::approx_tokens(&item.text) + 5
}

/// Marks `propose_core` on the best candidates that fit `budget`: highest score first, then the
/// cheaper rule, then document order. The proposal can never overflow the session start.
pub fn select_core(items: &mut [ImportItem], budget: usize) {
    let mut order: Vec<usize> = (0..items.len())
        .filter(|&i| items[i].core_score.is_some_and(|s| s >= CORE_MIN_SCORE))
        .collect();
    order.sort_by(|&a, &b| {
        let (ia, ib) = (&items[a], &items[b]);
        ib.core_score
            .partial_cmp(&ia.core_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(core_cost(ia).cmp(&core_cost(ib)))
            .then(ia.line.cmp(&ib.line))
    });
    let mut used = CORE_HEADER_TOKENS;
    for item in items.iter_mut() {
        item.propose_core = false;
    }
    for i in order {
        let cost = core_cost(&items[i]);
        if used + cost <= budget {
            used += cost;
            items[i].propose_core = true;
        }
    }
}

/// A repository's file list, answering "which path does this mention name?".
pub struct RepoFiles {
    files: Vec<String>,
    dirs: std::collections::HashSet<String>,
}

impl RepoFiles {
    pub fn new(files: impl IntoIterator<Item = String>) -> Self {
        let files: Vec<String> = files.into_iter().map(|f| spatial::normalize(&f)).filter(|f| !f.is_empty()).collect();
        let dirs = files
            .iter()
            .flat_map(|f| spatial::ancestors(f).into_iter().skip(1))
            .filter(|d| !d.is_empty())
            .collect();
        RepoFiles { files, dirs }
    }

    /// The path itself if it is a file or folder of the repo; for a bare name (`binary.rs`), the
    /// one file with that name, if there is exactly one.
    pub fn resolve(&self, p: &str) -> Option<String> {
        if self.dirs.contains(p) || self.files.iter().any(|f| f == p) {
            return Some(p.to_string());
        }
        if p.contains('/') {
            return None;
        }
        let mut matches = self.files.iter().filter(|f| f.rsplit('/').next() == Some(p));
        match (matches.next(), matches.next()) {
            (Some(only), None) => Some(only.clone()),
            _ => None,
        }
    }
}

/// Imports a Markdown document into `m` using `"CLAUDE.md"` as the default source filename for `node.key`.
pub fn import_markdown(
    m: &mut Memory,
    md: &str,
    branch: &str,
    commit: &str,
    now: f64,
    apply_core: bool,
    resolve: impl Fn(&str) -> Option<String>,
) -> ImportReport {
    import_markdown_named(m, "CLAUDE.md", md, branch, commit, now, apply_core, resolve)
}

/// Imports a Markdown document into `m`, stamping each new node's `key` with `"<source>:L<line>"`.
/// Re-importing the same file preserves existing `node.key` values.
#[allow(clippy::too_many_arguments)]
pub fn import_markdown_named(
    m: &mut Memory,
    source: &str,
    md: &str,
    branch: &str,
    commit: &str,
    now: f64,
    apply_core: bool,
    resolve: impl Fn(&str) -> Option<String>,
) -> ImportReport {
    let mut items = parse_markdown(md, resolve);
    let view = View::on_branch(f64::INFINITY, branch);
    let scores = score::score_all(m, &view);
    let mut report = ImportReport::default();
    // Core rules already confirmed keep their place; the proposal fills what is left.
    let used: usize = m
        .nodes
        .iter()
        .filter(|n| n.has(flags::CORE) && scores[n.id as usize].visible)
        .map(|n| text::approx_tokens(super::hook::injected_text(n)) + 5)
        .sum();
    if used > 0 {
        select_core(&mut items, BUDGET_CORE.saturating_sub(used));
    }

    for item in items {
        let item_key = format!("{source}:L{}", item.line);
        let toks = text::tokens(&item.text);
        if let Some(id) = find_existing(m, &scores, &toks, &item.path) {
            if !report.added.contains(&id) && !report.existing.contains(&id) {
                report.existing.push(id);
            }
            if m.nodes[id as usize].key.is_empty() {
                m.nodes[id as usize].key = item_key;
            } else if m.nodes[id as usize].key != item_key {
                report.merged.push((item.line, id));
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
        let full_text = with_context(&item);
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
            key: item_key,
            label: text::clip(&item.text, 64),
            // The context travels with the text: it is what the agent reads and what lexical recall matches.
            text: text::clip(&full_text, 600),
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

fn with_context(item: &ImportItem) -> String {
    if item.context.is_empty() {
        item.text.clone()
    } else {
        format!("[{}] {}", item.context, item.text)
    }
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

/// One bullet or paragraph, with where it sits in the document.
struct Block {
    /// 1-indexed first line.
    line: usize,
    raw: String,
    bullet: bool,
    /// Leading spaces of its first line (nested bullets are deeper).
    indent: usize,
    /// Headings above it, level ≥ 2 (the `#` title says nothing about one rule).
    headings: Vec<String>,
}

/// Splits Markdown into blocks, skipping code fences, headings and tables.
fn split_blocks(md: &str) -> Vec<Block> {
    let md = md.strip_prefix('\u{feff}').unwrap_or(md);
    let mut blocks: Vec<Block> = Vec::new();
    let mut cur: Option<Block> = None;
    let mut in_fence = false;
    let mut headings: Vec<(usize, String)> = Vec::new();

    let flush = |blocks: &mut Vec<Block>, cur: &mut Option<Block>| {
        if let Some(mut b) = cur.take() {
            b.raw = b.raw.trim().to_string();
            if !b.raw.is_empty() {
                blocks.push(b);
            }
        }
    };

    for (idx, line) in md.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed_start = line.trim_start_matches([' ', '\t', '\u{feff}']);
        let indent = line.len() - trimmed_start.len();
        if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
            flush(&mut blocks, &mut cur);
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let trimmed = trimmed_start.trim_end();
        let level = trimmed.bytes().take_while(|&c| c == b'#').count();
        if level > 0 && trimmed[level..].starts_with(' ') {
            flush(&mut blocks, &mut cur);
            headings.retain(|(l, _)| *l < level);
            headings.push((level, clean_markdown(trimmed[level..].trim())));
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with('|')
            || is_horizontal_rule(trimmed)
            || is_html_tag(trimmed)
            || trimmed.starts_with("> [!")
        {
            flush(&mut blocks, &mut cur);
            continue;
        }
        let content = trimmed.strip_prefix("> ").unwrap_or(trimmed).trim_start();
        let new_block = |raw: &str, bullet: bool| Block {
            line: line_no,
            raw: raw.to_string(),
            bullet,
            indent,
            headings: headings.iter().filter(|(l, _)| *l >= 2).map(|(_, h)| h.clone()).collect(),
        };
        if let Some(item) = strip_bullet(content) {
            flush(&mut blocks, &mut cur);
            cur = Some(new_block(item, true));
        } else if let Some(b) = cur.as_mut() {
            b.raw.push(' ');
            b.raw.push_str(content);
        } else {
            cur = Some(new_block(content, false));
        }
    }
    flush(&mut blocks, &mut cur);
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
/// Code paths win over documents; a document anchors only a block that is about it.
fn extract_location(raw: &str, cleaned: &str, resolve: &impl Fn(&str) -> Option<String>) -> (String, String) {
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

    let first_words: Vec<&str> = cleaned.split_whitespace().take(2).collect();
    let mut best: Option<(String, String)> = None;
    let mut doc: Option<(String, String)> = None;
    for (raw_cand, explicit) in candidates {
        let Some((path, sym)) = parse_candidate(&raw_cand, explicit, resolve) else {
            continue;
        };
        if is_doc(&path) {
            if doc.is_none() && is_about(&first_words, &path, resolve) {
                doc = Some((path, sym));
            }
            continue;
        }
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
    best.or(doc).unwrap_or_default()
}

/// Documentation, not code: mentioning it usually means "see" or "update", not "applies here".
fn is_doc(p: &str) -> bool {
    let lower = p.to_lowercase();
    lower.starts_with("docs/") || [".md", ".markdown", ".txt", ".rst", ".adoc"].iter().any(|e| lower.ends_with(e))
}

/// The block opens with the document (`` `README.md`: … ``) or with "En/In <documento>".
fn is_about(first_words: &[&str], path: &str, resolve: &impl Fn(&str) -> Option<String>) -> bool {
    let names_it = |w: &str| parse_candidate(w.trim_end_matches(':'), true, resolve).is_some_and(|(p, _)| p == path);
    match first_words {
        [w, ..] if names_it(w) => true,
        [w, next, ..] if matches!(text::fold(w).trim(), "en" | "in") => names_it(next),
        _ => false,
    }
}

fn parse_candidate(raw: &str, explicit: bool, resolve: &impl Fn(&str) -> Option<String>) -> Option<(String, String)> {
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
    if !is_valid_repo_path(&norm) {
        return None;
    }
    let path = resolve(&norm)?;
    let sym = if !sym_part.is_empty() && sym_part.chars().all(|c| c.is_alphanumeric() || c == '_') {
        sym_part.to_string()
    } else {
        String::new()
    };
    Some((path, sym))
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
            || text::contains_phrase(&folded, "cargo")
            || text::contains_phrase(&folded, "npm")
            || text::contains_phrase(&folded, "pytest"))
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
    let item = ImportItem {
        line: 0,
        text: cleaned.to_string(),
        context: String::new(),
        kind,
        path: path.to_string(),
        symbol: String::new(),
        emphasis: false,
        core_score: None,
        propose_core: false,
    };
    core_score(&item).is_some_and(|s| s >= CORE_MIN_SCORE)
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
    }) || ["tiene que", "tienen que", "hay que", "have to", "has to"].iter().any(|p| text::contains_phrase(folded, p))
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

    fn repo(p: &str) -> Option<String> {
        repo_exists(p).then(|| p.to_string())
    }

    #[test]
    fn splits_markdown_deduces_existing_paths_and_proposes_core() {
        let items = parse_markdown(SAMPLE_MD, repo);
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
        let r1 = import_markdown(&mut m, SAMPLE_MD, "", "a1b2c3d", 1000.0, false, repo);
        assert_eq!(r1.added.len(), 7);
        assert!(r1.existing.is_empty());
        assert_eq!(r1.proposed_core.len(), 2);
        assert!(r1.marked_core.is_empty());
        assert!(m.nodes.iter().all(|n| n.origin == Origin::Imported));
        assert!(m.nodes.iter().all(|n| !n.has(flags::CORE)));

        let scores_before = score_all(&m, &View::all());

        // Second import without `--yes`: zero duplicates, χ does not inflate, proposals remain.
        let r2 = import_markdown(&mut m, SAMPLE_MD, "", "a1b2c3d", 1005.0, false, repo);
        assert!(r2.added.is_empty(), "must not duplicate nodes");
        assert_eq!(r2.existing.len(), 7);
        assert_eq!(r2.proposed_core, r1.proposed_core);
        assert_eq!(m.nodes.len(), 7);
        let scores_after = score_all(&m, &View::all());
        for i in 0..m.nodes.len() {
            assert!((scores_before[i].chi - scores_after[i].chi).abs() < 1e-6);
        }

        // Confirming with `apply_core = true` (`--yes`) marks only the proposed nodes as CORE.
        let r3 = import_markdown(&mut m, SAMPLE_MD, "", "a1b2c3d", 1010.0, true, repo);
        assert!(r3.added.is_empty());
        assert_eq!(r3.marked_core, r1.proposed_core);
        for &id in &r3.marked_core {
            assert!(m.nodes[id as usize].has(flags::CORE));
            assert!(m.nodes[id as usize].path.is_empty());
        }
    }

    #[test]
    fn importing_twice_preserves_node_keys() {
        let mut m = Memory::default();
        let r1 = import_markdown_named(&mut m, "CLAUDE.md", SAMPLE_MD, "", "a1b2c3d", 1000.0, false, repo);
        assert_eq!(r1.added.len(), 7);
        let keys_first: Vec<String> = m.nodes.iter().map(|n| n.key.clone()).collect();
        assert_eq!(
            keys_first,
            vec![
                "CLAUDE.md:L3",
                "CLAUDE.md:L11",
                "CLAUDE.md:L12",
                "CLAUDE.md:L13",
                "CLAUDE.md:L14",
                "CLAUDE.md:L15",
                "CLAUDE.md:L16",
            ]
        );

        // Re-importing the exact same file keeps the exact same keys and reports zero merges.
        let r2 = import_markdown_named(&mut m, "CLAUDE.md", SAMPLE_MD, "", "a1b2c3d", 1005.0, true, repo);
        let keys_second: Vec<String> = m.nodes.iter().map(|n| n.key.clone()).collect();
        assert_eq!(keys_first, keys_second);
        assert!(r2.merged.is_empty());
    }

    #[test]
    fn context_bare_names_documents_and_lead_ins() {
        let md = "# Título

## Fase 2 · `cthg import`

- Importar dos veces no duplica nodos.

## Cómo trabajar

4. Al terminar una fase:
   - pasa las dos comprobaciones;
5. No hagas commits ni push.

## Reglas

- Formato binario: sube `VERSION` en `binary.rs` y actualiza `docs/formato.md`.
- Si cambias una constante, actualiza GUIA.md en el mismo trabajo.
- En `README.md` los títulos van en español.
- Una fase por encargo, en orden: primero `import`.
";
        let files = RepoFiles::new(
            ["crates/core/src/binary.rs", "crates/cli/src/main.rs", "docs/formato.md", "GUIA.md", "README.md"].map(String::from),
        );
        let items = parse_markdown(md, |p| files.resolve(p));
        let find = |s: &str| items.iter().find(|i| i.text.contains(s)).unwrap();

        // A: the section travels with the rule.
        assert_eq!(find("dos veces").context, "Fase 2 · `cthg import`");
        // D: a sub-bullet keeps its lead-in; the next sibling does not inherit it.
        assert_eq!(find("comprobaciones").context, "Cómo trabajar › Al terminar una fase");
        assert_eq!(find("No hagas commits").context, "Cómo trabajar");
        // B: a bare file name resolves to its one file.
        assert_eq!(find("Formato binario").path, "crates/core/src/binary.rs");
        // C: mentioning a document does not anchor; a block about the document does.
        assert_eq!(find("constante").path, "");
        assert_eq!(find("títulos").path, "README.md");
        // "encargo" contains "cargo " but is not a command.
        assert_eq!(find("encargo").kind, NodeKind::Rule);

        // The context is part of what is stored and matched, not of what is compared on re-import.
        let mut m = Memory::default();
        let r1 = import_markdown_named(&mut m, "A.md", md, "", "", 1.0, false, |p| files.resolve(p));
        let n = &m.nodes[r1.added[0] as usize];
        assert!(n.text.starts_with("[Fase 2 · `cthg import`] Importar"), "{}", n.text);
        assert_eq!(n.label, "Importar dos veces no duplica nodos.");
        let r2 = import_markdown_named(&mut m, "A.md", md, "", "", 2.0, false, |p| files.resolve(p));
        assert!(r2.added.is_empty() && r2.merged.is_empty());
    }

    #[test]
    fn bare_names_need_a_unique_match() {
        let files = RepoFiles::new(["a/main.rs", "b/main.rs", "a/lib.rs"].map(String::from));
        assert_eq!(files.resolve("lib.rs").as_deref(), Some("a/lib.rs"));
        assert_eq!(files.resolve("main.rs"), None, "two candidates: ambiguous");
        assert_eq!(files.resolve("a").as_deref(), Some("a"), "folders exist too");
        assert_eq!(files.resolve("c/lib.rs"), None);
    }

    #[test]
    fn core_proposal_is_ranked_and_fits_the_budget() {
        let mut md = String::from("## Reglas\n\n- **Nunca hagas push directo a main.**\n- Siempre comenta en inglés.\n- Prefiere funciones pequeñas.\n\n## Fase 4 · `cthg hook`\n\n- Nunca bloquees al usuario desde un hook.\n\n## Larga\n\n");
        for i in 0..40 {
            md.push_str(&format!("- Comprueba alfa{i} beta{i} gamma{i} delta{i} epsilon{i} zeta{i} theta{i} kappa{i} lambda{i} mu{i}.\n"));
        }
        let items = parse_markdown(&md, |_| None);
        let find = |s: &str| items.iter().find(|i| i.text.contains(s)).unwrap();
        assert_eq!(find("push directo").core_score, Some(3.0), "prohibition + emphasis");
        assert_eq!(find("comenta en inglés").core_score, Some(1.5), "obligation");
        assert_eq!(find("bloquees").core_score, None, "a section about one command is not general");
        assert!(find("push directo").propose_core && find("comenta en inglés").propose_core);
        assert!(!find("bloquees").propose_core);
        let cost: usize = items.iter().filter(|i| i.propose_core).map(core_cost).sum();
        assert!(cost + CORE_HEADER_TOKENS <= BUDGET_CORE, "proposal fits: {cost}");
        assert!(items.iter().filter(|i| i.core_score.is_some()).count() > items.iter().filter(|i| i.propose_core).count(), "some left out");
    }
}
