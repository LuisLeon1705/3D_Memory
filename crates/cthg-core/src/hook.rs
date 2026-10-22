//! Claude Code lifecycle hooks (`cthg hook <session-start|prompt|read|edit|stop>`; `stop` learns
//! the user's corrections, see `correction`).
//!
//! Rules:
//! - Never block or fail out loud: on any error, exit with code 0 and print nothing.
//! - Per-session deduplication in `<repo>/.cthg/sesion/<session_id>.json`: a memory node injected
//!   once in a session is not injected again in that same session.
//! - Dynamic token budgets (`Budgets::for_size`, GUIA §4.4): grow with the memory size `S`,
//!   level off at a ceiling, and cap the whole conversation at `max(S, 2 000)`; a small memory
//!   (`S ≤ WHOLE_MAX`) is injected whole at session start.
//! - `PostToolUse` (`read` and `edit`) wraps injected context in `hookSpecificOutput.additionalContext` JSON.

use super::index::ProjectIndex;
use super::learn;
use std::collections::HashMap;
use super::model::*;
use super::retrieve::{self, Query};
use super::score::{self, certainty_tag, Score, View};
use super::spatial::{self, Target};
use super::text;
use std::fs;
use std::path::{Path, PathBuf};

/// Budgets grow with the memory and level off: `B(S) = S·c / (c + S)` (GUIA §4.4).
/// A small memory is injected almost whole; a large one never past the ceiling `c`.
pub const CORE_CEIL: f32 = 1500.0;
pub const PROMPT_CEIL: f32 = 2000.0;
pub const READ_CEIL: f32 = 600.0;
/// Useful words at which a message gets half of its prompt budget: the more a message already
/// says, the less memory it needs ("arréglalo" needs the most).
pub const SPECIFICITY_HALF: f32 = 12.0;
/// However long the conversation, the project memory injects at most `max(S, this)` tokens:
/// never more than the static file it replaces (and a little room for tiny projects).
pub const CONVERSATION_FLOOR: usize = 2000;
/// The global layer has its own budget; it is never taken from the project's.
pub const GLOBAL_CORE_MAX: usize = 600;
/// Below these sizes a memory is injected whole: it costs less than deciding what to leave out.
pub const CORE_FLOOR: usize = 300;
pub const PROMPT_FLOOR: usize = 300;
pub const READ_FLOOR: usize = 150;
/// Up to this `S`, `core_context` injects every injectable node (not just `CORE`) at session start:
/// `CORE` first, then the rest by χ, within `max(budgets.core, S)`.
pub const WHOLE_MAX: usize = 300;
/// Fixed part of an injected line (`- #12 regla (ruta): `, its share of the band headers).
const LINE_OVERHEAD: usize = 6;

/// `max( min(S, floor), S·c / (c + S) )`, rounded: whole below the floor, then levelling off.
pub fn saturate(size: usize, ceil: f32, floor: usize) -> usize {
    let s = size as f32;
    let curve = (s * ceil / (ceil + s)).round() as usize;
    curve.max(size.min(floor))
}

/// Token budgets for one memory, from its size `S`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budgets {
    /// `S`: what the injectable memories would take written out as a static file.
    pub size: usize,
    pub core: usize,
    /// Per message, before the specificity factor (`for_prompt`).
    pub prompt: usize,
    pub read: usize,
    /// Total the project memory may inject in one conversation.
    pub conversation: usize,
}

impl Budgets {
    pub fn for_size(size: usize) -> Self {
        Budgets {
            size,
            core: saturate(size, CORE_CEIL, CORE_FLOOR),
            prompt: saturate(size, PROMPT_CEIL, PROMPT_FLOOR),
            read: saturate(size, READ_CEIL, READ_FLOOR),
            conversation: size.max(CONVERSATION_FLOOR),
        }
    }

    pub fn of(m: &Memory) -> Self {
        Self::for_size(memory_size(m))
    }

    /// This message's budget: `prompt · h / (h + q)`, `q` = its useful words.
    pub fn for_prompt(&self, prompt: &str) -> usize {
        let q = text::tokens(prompt).len() as f32;
        (self.prompt as f32 * SPECIFICITY_HALF / (SPECIFICITY_HALF + q)).round() as usize
    }
}

/// `S`: what injecting every memory would cost (not plans, not obsolete, not session logs):
/// its text, its path and the fixed part of the line. Proposals count: reading their files
/// injects them, and without them a small memory would leave no room for them there.
pub fn memory_size(m: &Memory) -> usize {
    m.nodes
        .iter()
        .filter(|n| !n.has(flags::POTENTIAL) && !n.has(flags::STALE) && n.kind != NodeKind::Session)
        .map(|n| line_cost(injected_text(n), &n.path))
        .sum()
}

/// Tokens one memory takes when injected.
pub fn line_cost(text: &str, path: &str) -> usize {
    text::approx_tokens(text) + text::approx_tokens(path) + LINE_OVERHEAD
}

fn tokens_of(m: &Memory, scores: &[Score], ids: &[u32]) -> usize {
    ids.iter().map(|&id| text::approx_tokens(&render_node(m, scores, &m.nodes[id as usize]))).sum()
}

/// A proposal (`PROPOSED`, a correction captured by heuristics and not confirmed) is injected
/// only when one of its files is read (`read`): anchored at that file or at a folder holding it.
/// Never in the core, never from a message, never pulled in by its neighbours.
pub fn may_inject(n: &Node, score: &Score, read: Option<&str>) -> bool {
    if score.superseded {
        return false;
    }
    !n.has(flags::PROPOSED) || read.is_some_and(|p| !n.path.is_empty() && spatial::is_ancestor_or_self(&n.path, p))
}

/// What the project memory may still inject in this conversation.
fn remaining(b: &Budgets, session: &SessionState) -> usize {
    b.conversation.saturating_sub(session.spent as usize)
}

/// Parsed fields from Claude Code's hook `stdin` JSON payload.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookInput {
    pub session_id: String,
    pub cwd: String,
    pub prompt: String,
    pub file_path: String,
    /// `Stop`: the conversation so far, where the user's corrections are read.
    pub transcript_path: String,
}

impl HookInput {
    /// Extracts `session_id`, `cwd`, `prompt`, `tool_input.file_path` and `transcript_path` from a JSON payload.
    pub fn parse(json: &str) -> Self {
        let session_id = extract_str(json, "session_id").unwrap_or_else(|| "default".to_string());
        let cwd = extract_str(json, "cwd").unwrap_or_default();
        let prompt = extract_str(json, "prompt").unwrap_or_default();
        let file_path = extract_nested_str(json, "tool_input", "file_path")
            .or_else(|| extract_nested_str(json, "tool_input", "path"))
            .or_else(|| extract_str(json, "file_path"))
            .unwrap_or_default();
        HookInput {
            session_id: sanitize_session_id(&session_id),
            cwd,
            prompt,
            file_path,
            transcript_path: extract_str(json, "transcript_path").unwrap_or_default(),
        }
    }
}

/// Per-session state persisted at `<repo>/.cthg/sesion/<session_id>.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionState {
    pub session_id: String,
    pub injected: Vec<u32>,
    /// Agents that only give a transcript (Antigravity): bytes of it already read.
    pub offset: u64,
    /// Step index of the last user message handled (a new one starts a new turn).
    pub turn: i64,
    /// The once-per-conversation forgetting step already ran.
    pub started: bool,
    /// Tokens the project memory injected in this conversation (capped by `Budgets::conversation`).
    pub spent: u64,
    /// Rule files the agent already loads by itself (`CLAUDE.md`, `AGENTS.md`…): memories
    /// imported from them are never injected again. Set by the caller; not saved.
    pub native: Vec<String>,
}

impl SessionState {
    pub fn new(session_id: &str) -> Self {
        SessionState {
            session_id: sanitize_session_id(session_id),
            injected: Vec::new(),
            offset: 0,
            turn: -1,
            started: false,
            spent: 0,
            native: Vec::new(),
        }
    }

    pub fn load(dir: &Path, session_id: &str) -> Self {
        let sid = sanitize_session_id(session_id);
        let path = Self::file_path(dir, &sid);
        let Ok(raw) = fs::read_to_string(&path) else {
            return Self::new(&sid);
        };
        Self::decode(&raw).unwrap_or_else(|| Self::new(&sid))
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        let path = Self::file_path(dir, &self.session_id);
        fs::write(path, self.encode())
    }

    pub fn file_path(dir: &Path, session_id: &str) -> PathBuf {
        dir.join(format!("{}.json", sanitize_session_id(session_id)))
    }

    pub fn was_injected(&self, id: u32) -> bool {
        self.injected.contains(&id)
    }

    /// Imported from a root rule file the agent loads by itself (`key` = `AGENTS.md:L12`).
    pub fn is_native(&self, n: &Node) -> bool {
        n.key.rsplit_once(":L").is_some_and(|(file, _)| self.native.iter().any(|f| f == file))
    }

    pub fn record(&mut self, ids: &[u32]) {
        for &id in ids {
            if !self.injected.contains(&id) {
                self.injected.push(id);
            }
        }
    }

    pub fn encode(&self) -> String {
        let nums: Vec<String> = self.injected.iter().map(|n| n.to_string()).collect();
        format!(
            "{{\"session_id\":\"{}\",\"injected\":[{}],\"offset\":{},\"turn\":{},\"started\":{},\"spent\":{}}}\n",
            escape_json(&self.session_id),
            nums.join(","),
            self.offset,
            self.turn,
            self.started,
            self.spent
        )
    }

    pub fn decode(json: &str) -> Option<Self> {
        let v = crate::json::Json::parse(json.trim())?;
        let session_id = v.get("session_id").and_then(|x| x.str()).unwrap_or("default");
        let num = |k: &str| v.get(k).and_then(|x| x.num());
        Some(SessionState {
            session_id: sanitize_session_id(session_id),
            injected: v.get("injected").map(|a| a.arr().iter().filter_map(|x| x.num()).map(|n| n as u32).collect()).unwrap_or_default(),
            offset: num("offset").unwrap_or(0.0) as u64,
            turn: num("turn").unwrap_or(-1.0) as i64,
            started: v.get("started") == Some(&crate::json::Json::Bool(true)),
            spent: num("spent").unwrap_or(0.0) as u64,
            native: Vec::new(),
        })
    }
}

/// Converts an absolute or relative `file_path` into a repository-relative normalized path.
/// Returns `""` if the path lies outside `repo_root`.
pub fn relativize_path(repo_root: &Path, raw_path: &str) -> String {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let root_norm = spatial::normalize(&repo_root.to_string_lossy());
    let file_norm = spatial::normalize(trimmed);
    if root_norm.is_empty() {
        return file_norm;
    }
    let root_lower = root_norm.to_lowercase();
    let file_lower = file_norm.to_lowercase();
    if file_lower == root_lower {
        return String::new();
    }
    if file_lower.starts_with(&root_lower) && file_norm.as_bytes().get(root_norm.len()) == Some(&b'/') {
        return spatial::normalize(&file_norm[root_norm.len() + 1..]);
    }
    if Path::new(trimmed).is_absolute() {
        return String::new();
    }
    file_norm
}

/// Formats a `PostToolUse` JSON response with `hookSpecificOutput.additionalContext`.
pub fn format_post_tool_use(additional_context: &str) -> String {
    format!(
        "{{\"hookSpecificOutput\":{{\"hookEventName\":\"PostToolUse\",\"additionalContext\":\"{}\"}}}}",
        escape_json(additional_context)
    )
}

/// Renders one memory node line with optional neighbor origin: `- #id tipo (ruta, vecino de origen): texto ← causa`.
pub fn render_node_with_neighbor(
    m: &Memory,
    scores: &[Score],
    n: &Node,
    neighbor_of: Option<&str>,
) -> String {
    let mut s = format!("- #{} {}", n.id, n.kind.name());
    if !n.path.is_empty() {
        s.push_str(" (");
        s.push_str(&n.path);
        if !n.symbol.is_empty() {
            s.push_str(&format!("::{}", n.symbol));
        }
        if let Some(origin) = neighbor_of {
            s.push_str(&format!(", vecino de {origin}"));
        }
        s.push(')');
    }
    s.push_str(&format!(": {}", text::clip(injected_text(n), 240)));
    let cause = m
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Causes && e.to == n.id && scores[e.from as usize].visible)
        .max_by(|a, b| {
            scores[a.from as usize]
                .chi
                .partial_cmp(&scores[b.from as usize].chi)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|e| {
            let mut from = e.from;
            let mut visited = std::collections::HashSet::new();
            visited.insert(from);
            while let Some(sub) = m.edges.iter().find(|s| {
                s.to == from
                    && s.kind == EdgeKind::Supersedes
                    && !visited.contains(&s.from)
                    && scores.get(s.from as usize).is_some_and(|sc| sc.visible && sc.chi > 0.0)
            }) {
                from = sub.from;
                visited.insert(from);
                if visited.len() > 32 {
                    break;
                }
            }
            &m.nodes[from as usize]
        });
    if let Some(c) = cause {
        s.push_str(&format!(" ← {}", text::clip(&c.label, 80)));
    }
    s
}

/// Renders one memory node line: `- #id tipo (ruta::simbolo): texto ← causa`.
pub fn render_node(m: &Memory, scores: &[Score], n: &Node) -> String {
    render_node_with_neighbor(m, scores, n, None)
}

/// What a node shows when injected. Imported text carries its section as `[Sección › Introducción] …`
/// for matching; injected, the section is kept only for fragments too short to stand alone
/// ("pasa las dos comprobaciones"), because on every line it costs ~20 % more tokens.
pub fn injected_text(n: &Node) -> &str {
    match n.text.strip_prefix('[').and_then(|t| t.split_once("] ")) {
        Some((_, body)) if body.chars().count() >= SELF_CONTAINED_CHARS => body,
        _ => &n.text,
    }
}

/// Below this length a rule needs its section to be understood.
const SELF_CONTAINED_CHARS: usize = 40;

/// Formats `ids` grouped by χ certainty band under `heading`, rendering neighbor indicators if present.
pub fn format_bands_with_neighbors(
    heading: &str,
    m: &Memory,
    scores: &[Score],
    ids: &[u32],
    neighbor_map: &HashMap<u32, String>,
) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let mut out = String::from(heading);
    let bands = ["verificado", "probable", "sin evidencia", "dudoso", "descartado"];
    for band in bands {
        let lines: Vec<String> = ids
            .iter()
            .filter(|&&id| certainty_tag(scores[id as usize].chi) == band)
            .map(|&id| {
                let neighbor_of = neighbor_map.get(&id).map(String::as_str);
                render_node_with_neighbor(m, scores, &m.nodes[id as usize], neighbor_of)
            })
            .collect();
        if lines.is_empty() {
            continue;
        }
        let note = if band == "descartado" { " (ya se probó o se sustituyó: no repetir)" } else { "" };
        out.push_str(&format!("\n[{band}]{note}"));
        for l in lines {
            out.push('\n');
            out.push_str(&l);
        }
    }
    out
}

/// Formats `ids` grouped by χ certainty band under `heading`.
pub fn format_bands(heading: &str, m: &Memory, scores: &[Score], ids: &[u32]) -> String {
    format_bands_with_neighbors(heading, m, scores, ids, &HashMap::new())
}

/// Runs `session-start`: one forgetting step, then injects `CORE` nodes within `Budgets::core`.
pub fn handle_session_start(m: &mut Memory, view: &View, session: &mut SessionState) -> String {
    handle_session_start_layered(m, view, session, None)
}

/// The user's global `CORE` (rules for every project), within `budget`. Read-only: the global
/// memory is shared by every project, so a session never changes it. Returns the text, the
/// tokens it used, tokenized rules, and the injected node ids. Global ids print as `#g3`.
pub fn global_core(global: &Memory, budget: usize) -> (String, usize, Vec<Vec<String>>) {
    let (out, used, toks, _) = global_core_with_ids(global, budget);
    (out, used, toks)
}

pub fn global_core_with_ids(global: &Memory, budget: usize) -> (String, usize, Vec<Vec<String>>, Vec<u32>) {
    let scores = score::score_all(global, &View::all());
    let all = retrieve::core(global, &scores);
    let ids = retrieve::fit_budget(global, &all, budget, |n| render_node(global, &scores, n));
    if ids.is_empty() {
        return (String::new(), 0, Vec::new(), Vec::new());
    }
    let used: usize = ids.iter().map(|&id| text::approx_tokens(&render_node(global, &scores, &global.nodes[id as usize]))).sum();
    let mut out = format_bands("## Reglas globales (siempre, en todos los proyectos)", global, &scores, &ids).replace("\n- #", "\n- #g");
    if ids.len() < all.len() {
        out.push_str(&format!("\n({} reglas globales no caben aquí; `cthg core --global` las muestra todas)", all.len() - ids.len()));
    }
    let toks = ids.iter().map(|&id| text::tokens(&global.nodes[id as usize].text)).collect();
    (out, used, toks, ids)
}

/// A project rule that says what a global rule already said is not injected twice.
pub const DUPLICATE_OF_GLOBAL: f32 = 0.5;

/// Name of the JSON file storing global rule recalls in `.cthg/`.
pub const GLOBAL_RECALLS_FILE: &str = "global_recalls.json";

/// Loads the counts of global rules injected in this repository from `.cthg/global_recalls.json`.
pub fn load_global_recalls(cthg_dir: &Path) -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    let path = cthg_dir.join(GLOBAL_RECALLS_FILE);
    let Ok(content) = fs::read_to_string(&path) else { return map };
    if let Some(crate::json::Json::Obj(entries)) = crate::json::Json::parse(&content) {
        for (k, v) in entries {
            if let (Ok(id), Some(num)) = (k.parse::<u32>(), v.num()) {
                map.insert(id, num as u32);
            }
        }
    }
    map
}

/// Records that `ids` from the global memory were injected into an agent session in this repository.
/// Increments their counters in `.cthg/global_recalls.json`.
pub fn record_global_recalls(cthg_dir: &Path, ids: &[u32]) {
    if ids.is_empty() {
        return;
    }
    let mut counts = load_global_recalls(cthg_dir);
    for &id in ids {
        *counts.entry(id).or_default() += 1;
    }
    let mut entries: Vec<(&u32, &u32)> = counts.iter().collect();
    entries.sort_by_key(|(k, _)| *k);
    let mut out = String::from("{\n");
    for (i, (k, v)) in entries.iter().enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        out.push_str(&format!("  \"{}\": {}", k, v));
    }
    out.push_str("\n}\n");
    let _ = fs::create_dir_all(cthg_dir);
    let _ = fs::write(cthg_dir.join(GLOBAL_RECALLS_FILE), out);
}

/// Session start with the global layer first: global `CORE`, then the project's `CORE` in the
/// budget left, skipping project rules that repeat a global one.
pub fn handle_session_start_layered(m: &mut Memory, view: &View, session: &mut SessionState, global: Option<&Memory>) -> String {
    handle_session_start_layered_with_global(m, view, session, global).0
}

pub fn handle_session_start_layered_with_global(m: &mut Memory, view: &View, session: &mut SessionState, global: Option<&Memory>) -> (String, Vec<u32>) {
    session.injected.clear();
    session.spent = 0;
    learn::begin_session(m);
    session.started = true;
    core_context_with_global(m, view, session, global)
}

/// The `CORE` text (global layer, then the project's) without starting a session: for agents
/// whose injected context only lasts one turn and must be given again on the next one.
/// When `S ≤ WHOLE_MAX`, a small memory goes in whole: `CORE` first, then the remaining
/// injectable nodes by χ, within `max(budgets.core, S)`.
pub fn core_context(m: &mut Memory, view: &View, session: &mut SessionState, global: Option<&Memory>) -> String {
    core_context_with_global(m, view, session, global).0
}

pub fn core_context_with_global(m: &mut Memory, view: &View, session: &mut SessionState, global: Option<&Memory>) -> (String, Vec<u32>) {
    let (global_out, _, global_toks, global_ids) = global.map(|g| global_core_with_ids(g, GLOBAL_CORE_MAX)).unwrap_or_default();
    let budgets = Budgets::of(m);
    let scores = score::score_all(m, view);
    let mut base = retrieve::core(m, &scores);
    if budgets.size <= WHOLE_MAX {
        let mut rest: Vec<u32> = m
            .nodes
            .iter()
            .filter(|n| {
                !n.has(flags::CORE)
                    && scores[n.id as usize].visible
                    && !n.has(flags::POTENTIAL)
                    && !n.has(flags::STALE)
                    && n.kind != NodeKind::Session
                    && !n.has(flags::PROPOSED)
            })
            .map(|n| n.id)
            .collect();
        rest.sort_by(|&a, &b| scores[b as usize].chi.partial_cmp(&scores[a as usize].chi).unwrap_or(std::cmp::Ordering::Equal));
        base.extend(rest);
    }
    let candidates: Vec<u32> = base
        .into_iter()
        .filter(|&id| !session.was_injected(id) && !session.is_native(&m.nodes[id as usize]) && may_inject(&m.nodes[id as usize], &scores[id as usize], None))
        .filter(|&id| {
            let toks = text::tokens(&m.nodes[id as usize].text);
            !global_toks.iter().any(|g| text::similarity(&toks, g) >= DUPLICATE_OF_GLOBAL)
        })
        .collect();
    let cap = if budgets.size <= WHOLE_MAX { budgets.core.max(budgets.size) } else { budgets.core };
    let budget = cap.min(remaining(&budgets, session));
    let ids = retrieve::fit_budget(m, &candidates, budget, |n| render_node(m, &scores, n));
    if ids.is_empty() {
        return (global_out, global_ids);
    }
    session.spent += tokens_of(m, &scores, &ids) as u64;
    session.record(&ids);
    for &id in &ids {
        if let Some(n) = m.node_mut(id) {
            n.activation = 1.0;
            n.recalls += 1;
        }
    }
    let mut out = format_bands("## Reglas del proyecto (siempre)", m, &scores, &ids);
    // Never drop core rules silently: the agent should know some are missing.
    let missing_core = candidates.iter().filter(|&&id| m.nodes[id as usize].has(flags::CORE) && !ids.contains(&id)).count();
    if missing_core > 0 {
        out.push_str(&format!("\n({missing_core} reglas del núcleo no caben aquí; `cthg core` las muestra todas)"));
    }
    let res = if global_out.is_empty() {
        out
    } else {
        format!("{global_out}\n\n{out}")
    };
    (res, global_ids)
}

/// Runs `prompt` with optional code index: fuses lexical, git spatial, neighbor expansion,
/// and message identifier matching within `Budgets::for_prompt`.
pub fn handle_prompt_with_index(
    m: &mut Memory,
    view: &View,
    session: &mut SessionState,
    prompt: &str,
    git_targets: Vec<Target>,
    index: Option<&ProjectIndex>,
) -> String {
    handle_prompt_with_weights(
        m,
        view,
        session,
        prompt,
        git_targets,
        index,
        spatial::W_NEIGHBOR,
        spatial::W_SYMBOL,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn handle_prompt_with_weights(
    m: &mut Memory,
    view: &View,
    session: &mut SessionState,
    prompt: &str,
    git_targets: Vec<Target>,
    index: Option<&ProjectIndex>,
    w_neighbor: f32,
    w_symbol: f32,
) -> String {
    let scores = score::score_all(m, view);
    let mut all_targets = spatial::expand_neighbors(&git_targets, index, w_neighbor, spatial::MAX_NEIGHBORS);
    let id_targets = spatial::identifier_targets(prompt, index, w_symbol, spatial::MIN_IDENT_LEN, spatial::MAX_DEFS);
    spatial::merge_targets(&mut all_targets, id_targets);

    let q = Query {
        text: prompt.to_string(),
        targets: all_targets.clone(),
    };
    let candidates: Vec<u32> = retrieve::recall(m, &scores, &q, 40, -1.0)
        .into_iter()
        .map(|r| r.id)
        .filter(|&id| !session.was_injected(id) && !session.is_native(&m.nodes[id as usize]) && may_inject(&m.nodes[id as usize], &scores[id as usize], None))
        .collect();
    let budgets = Budgets::of(m);
    let budget = budgets.for_prompt(prompt).min(remaining(&budgets, session));
    let ids = retrieve::fit_budget(m, &candidates, budget, |n| render_node(m, &scores, n));
    if ids.is_empty() {
        return String::new();
    }
    session.spent += tokens_of(m, &scores, &ids) as u64;
    session.record(&ids);
    learn::co_recalled(m, &ids);
    let neighbor_map = spatial::node_neighbor_origins(m, &all_targets, &ids);
    format_bands_with_neighbors("## Memoria relevante", m, &scores, &ids, &neighbor_map)
}

/// Runs `prompt`: fuses lexical and `git status`/`git diff` spatial channels within the
/// message's budget (`Budgets::for_prompt`) and what is left of the conversation's.
pub fn handle_prompt(
    m: &mut Memory,
    view: &View,
    session: &mut SessionState,
    prompt: &str,
    git_targets: Vec<Target>,
) -> String {
    handle_prompt_with_index(m, view, session, prompt, git_targets, None)
}

/// Runs `read` with optional code index: just-in-time spatial recall for `rel_path` and its neighbors.
pub fn handle_read_with_index(
    m: &mut Memory,
    view: &View,
    session: &mut SessionState,
    rel_path: &str,
    index: Option<&ProjectIndex>,
) -> String {
    let context = read_context_with_index(m, view, session, rel_path, index);
    if context.is_empty() {
        return context;
    }
    format_post_tool_use(&context)
}

/// Runs `read`: just-in-time spatial recall for `rel_path` within `Budgets::read`, wrapped in
/// `PostToolUse` `hookSpecificOutput.additionalContext` JSON.
pub fn handle_read(m: &mut Memory, view: &View, session: &mut SessionState, rel_path: &str) -> String {
    handle_read_with_index(m, view, session, rel_path, None)
}

/// Shared just-in-time spatial recall for a file just read or written (`Budgets::read`, per-session
/// deduplication, proposals allowed for that file, optional neighbor expansion).
fn file_context_with_weights(
    m: &mut Memory,
    view: &View,
    session: &mut SessionState,
    rel_path: &str,
    suffix: &str,
    index: Option<&ProjectIndex>,
    w_neighbor: f32,
) -> String {
    let rel_path = spatial::normalize(rel_path);
    if rel_path.is_empty() {
        return String::new();
    }
    let scores = score::score_all(m, view);
    let base_targets = vec![Target::new(&rel_path, 1.0)];
    let targets = spatial::expand_neighbors(&base_targets, index, w_neighbor, spatial::MAX_NEIGHBORS);

    let q = Query {
        text: String::new(),
        targets: targets.clone(),
    };
    let candidates: Vec<u32> = retrieve::recall(m, &scores, &q, 40, -1.0)
        .into_iter()
        .map(|r| r.id)
        .filter(|&id| !session.was_injected(id) && !session.is_native(&m.nodes[id as usize]) && may_inject(&m.nodes[id as usize], &scores[id as usize], Some(&rel_path)))
        .collect();
    let budgets = Budgets::of(m);
    let budget = budgets.read.min(remaining(&budgets, session));
    let ids = retrieve::fit_budget(m, &candidates, budget, |n| render_node(m, &scores, n));
    if ids.is_empty() {
        return String::new();
    }
    session.spent += tokens_of(m, &scores, &ids) as u64;
    session.record(&ids);
    learn::co_recalled(m, &ids);
    let neighbor_map = spatial::node_neighbor_origins(m, &targets, &ids);
    let heading = format!("## Memoria relevante ({rel_path}{suffix})");
    format_bands_with_neighbors(&heading, m, &scores, &ids, &neighbor_map)
}

fn file_context_with_index(
    m: &mut Memory,
    view: &View,
    session: &mut SessionState,
    rel_path: &str,
    suffix: &str,
    index: Option<&ProjectIndex>,
) -> String {
    file_context_with_weights(m, view, session, rel_path, suffix, index, spatial::W_NEIGHBOR)
}

pub fn read_context_with_weights(
    m: &mut Memory,
    view: &View,
    session: &mut SessionState,
    rel_path: &str,
    index: Option<&ProjectIndex>,
    w_neighbor: f32,
) -> String {
    file_context_with_weights(m, view, session, rel_path, "", index, w_neighbor)
}

/// Reads plain context with optional code index for neighbor expansion.
pub fn read_context_with_index(
    m: &mut Memory,
    view: &View,
    session: &mut SessionState,
    rel_path: &str,
    index: Option<&ProjectIndex>,
) -> String {
    file_context_with_index(m, view, session, rel_path, "", index)
}

/// The plain context `read` injects (what `handle_read` wraps in JSON); `eval` counts its tokens.
pub fn read_context(m: &mut Memory, view: &View, session: &mut SessionState, rel_path: &str) -> String {
    read_context_with_index(m, view, session, rel_path, None)
}

/// Edits plain context with optional code index for neighbor expansion.
pub fn edit_context_with_index(
    m: &mut Memory,
    view: &View,
    session: &mut SessionState,
    rel_path: &str,
    index: Option<&ProjectIndex>,
) -> String {
    handle_edit(m, rel_path);
    file_context_with_index(m, view, session, rel_path, ", que acabas de escribir", index)
}

/// Boosts activation for `rel_path` (`handle_edit`) and injects any of that file's rules not yet
/// injected in this session, with the same `read` budget, deduplication and proposal rules.
pub fn edit_context(m: &mut Memory, view: &View, session: &mut SessionState, rel_path: &str) -> String {
    edit_context_with_index(m, view, session, rel_path, None)
}

/// Runs `edit`: boosts activation on nodes anchored at `rel_path` or its ancestor directories.
/// Returns how many nodes were activated.
pub fn handle_edit(m: &mut Memory, rel_path: &str) -> usize {
    let rel_path = spatial::normalize(rel_path);
    if rel_path.is_empty() {
        return 0;
    }
    let mut touched = 0;
    for n in m.nodes.iter_mut() {
        if !n.path.is_empty() && spatial::is_ancestor_or_self(&n.path, &rel_path) {
            n.activation = 1.0;
            touched += 1;
        }
    }
    touched
}

fn sanitize_session_id(s: &str) -> String {
    let clean: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    if clean.is_empty() {
        "default".to_string()
    } else {
        clean
    }
}

pub fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn extract_nested_str(json: &str, outer_key: &str, inner_key: &str) -> Option<String> {
    let needle = format!("\"{outer_key}\"");
    let pos = json.find(&needle)?;
    let after = &json[pos + needle.len()..];
    let brace = after.find('{')?;
    let sub = &after[brace..];
    extract_str(sub, inner_key)
}

fn extract_str(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut rest = json;
    while let Some(pos) = rest.find(&needle) {
        let after_key = &rest[pos + needle.len()..];
        let trimmed = after_key.trim_start();
        if let Some(after_colon) = trimmed.strip_prefix(':') {
            let val_start = after_colon.trim_start();
            if let Some(after_quote) = val_start.strip_prefix('"') {
                return parse_json_string(after_quote);
            }
        }
        rest = after_key;
    }
    None
}

fn parse_json_string(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000c}'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code) {
                            out.push(ch);
                        }
                    }
                }
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::{learn, Claim};

    #[test]
    fn parses_claude_hook_json_and_formats_post_tool_use() {
        let raw = r#"{
            "session_id": "sess-42",
            "cwd": "C:\\repo\\demo",
            "hook_event_name": "PostToolUse",
            "tool_name": "Read",
            "tool_input": {
                "file_path": "C:\\repo\\demo\\src\\core\\collider.rs"
            }
        }"#;
        let input = HookInput::parse(raw);
        assert_eq!(input.session_id, "sess-42");
        assert_eq!(input.cwd, r"C:\repo\demo");
        assert_eq!(input.file_path, r"C:\repo\demo\src\core\collider.rs");
        assert_eq!(relativize_path(Path::new(r"C:\repo\demo"), &input.file_path), "src/core/collider.rs");
        let stop = HookInput::parse(r#"{"session_id":"s","transcript_path":"C:\\u\\.claude\\projects\\p\\s.jsonl","hook_event_name":"Stop","stop_hook_active":false}"#);
        assert_eq!(stop.transcript_path, r"C:\u\.claude\projects\p\s.jsonl");

        let out = format_post_tool_use("## Memoria\n- #1 regla: No uses \"unwrap\"");
        assert!(out.starts_with(r#"{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"#));
        assert_eq!(extract_nested_str(&out, "hookSpecificOutput", "additionalContext").unwrap(), "## Memoria\n- #1 regla: No uses \"unwrap\"");
    }

    #[test]
    fn hooks_deduplicate_across_session_and_edit_boosts_activation() {
        let mut m = Memory::default();
        let mut core_claim = Claim::rule("Nunca hagas push directo a main", "");
        core_claim.core = true;
        let core_id = learn(&mut m, &core_claim, 100.0)[0].node;
        let col_id = learn(&mut m, &Claim::rule("No uses unwrap en el colisionador", "src/core/collider.rs"), 101.0)[0].node;
        let dir_id = learn(&mut m, &Claim::rule("Todo el núcleo es determinista", "src/core"), 102.0)[0].node;
        // Pad memory so S > WHOLE_MAX (300) to test large memory behavior where non-core rules are not injected at session start.
        learn(&mut m, &Claim::rule(&"alfa ".repeat(200), "src/padding_one"), 105.0);
        learn(&mut m, &Claim::rule(&"beta ".repeat(200), "src/padding_two"), 106.0);
        learn(&mut m, &Claim::rule(&"gamma ".repeat(200), "src/padding_three"), 107.0);

        let view = View::all();
        let mut session = SessionState::new("s-test");

        // 1. SessionStart decays activation and injects CORE once.
        let s_out = handle_session_start(&mut m, &view, &mut session);
        assert!(s_out.contains("Nunca hagas push directo a main"));
        assert!(session.was_injected(core_id));
        assert!(m.nodes[col_id as usize].activation < 1.0, "non-core decayed at session start");

        // 2. Reading `src/core/collider.rs` injects `#col_id` and `#dir_id` via PostToolUse JSON.
        let r_out1 = handle_read(&mut m, &view, &mut session, "src/core/collider.rs");
        assert!(r_out1.contains("hookSpecificOutput"));
        assert!(r_out1.contains("No uses unwrap en el colisionador"));
        assert!(r_out1.contains("Todo el núcleo es determinista"));
        assert!(session.was_injected(col_id) && session.was_injected(dir_id));

        // 3. Reading the same file again in the same session produces no duplicate output.
        let r_out2 = handle_read(&mut m, &view, &mut session, "src/core/collider.rs");
        assert!(r_out2.is_empty(), "already injected in this session");

        // 4. Prompt touching the same file also skips already-injected memories.
        let p_out = handle_prompt(&mut m, &view, &mut session, "arréglalo", vec![Target::new("src/core/collider.rs", 0.8)]);
        assert!(p_out.is_empty(), "deduplicated across channels within the same session");

        // 5. Decay activation manually, then run `edit` on `src/core/collider.rs` to verify activation boost.
        m.nodes[col_id as usize].activation = 0.4;
        m.nodes[dir_id as usize].activation = 0.4;
        let touched = handle_edit(&mut m, "src/core/collider.rs");
        assert_eq!(touched, 2);
        assert_eq!(m.nodes[col_id as usize].activation, 1.0);
        assert_eq!(m.nodes[dir_id as usize].activation, 1.0);

        // 6. SessionState round-trips through JSON.
        let encoded = session.encode();
        let decoded = SessionState::decode(&encoded).unwrap();
        assert_eq!(decoded, session);
    }

    #[test]
    fn session_start_says_when_core_rules_do_not_fit() {
        let mut m = Memory::default();
        for i in 0..40 {
            let words: Vec<String> = ["alfa", "beta", "gamma", "delta", "epsilon", "zeta", "theta", "kappa"].iter().map(|w| format!("{w}{i}")).collect();
            let mut c = Claim::rule(&words.join(" "), "");
            c.core = true;
            learn(&mut m, &c, 100.0 + i as f64);
        }
        let out = handle_session_start(&mut m, &View::all(), &mut SessionState::new("s"));
        assert!(out.contains("no caben aquí"), "{out}");
    }

    #[test]
    fn injected_text_drops_the_section_unless_the_rule_needs_it() {
        let mut m = Memory::default();
        let long = learn(&mut m, &Claim::rule("[Reglas del código] Código, identificadores y comentarios en inglés", ""), 1.0)[0].node;
        let short = learn(&mut m, &Claim::rule("[Cómo trabajar › Al terminar una fase] pasa las dos comprobaciones;", ""), 2.0)[0].node;
        assert_eq!(injected_text(&m.nodes[long as usize]), "Código, identificadores y comentarios en inglés");
        assert!(injected_text(&m.nodes[short as usize]).starts_with("[Cómo trabajar › Al terminar una fase]"));
    }

    #[test]
    fn global_core_goes_first_shares_the_budget_and_is_not_repeated() {
        let mut global = Memory::default();
        let mut g1 = Claim::rule("No hagas commits ni push sin que el usuario lo pida", "");
        g1.core = true;
        learn(&mut global, &g1, 1.0);
        let mut g2 = Claim::rule("Comentarios e identificadores siempre en inglés", "");
        g2.core = true;
        learn(&mut global, &g2, 2.0);

        let mut m = Memory::default();
        let mut dup = Claim::rule("No hagas commits ni push: el usuario revisa el diff", "");
        dup.core = true;
        let dup = learn(&mut m, &dup, 1.0)[0].node;
        let mut own = Claim::rule("cthg-core no tiene dependencias y debe seguir así", "");
        own.core = true;
        let own = learn(&mut m, &own, 2.0)[0].node;

        let mut session = SessionState::new("g");
        let out = handle_session_start_layered(&mut m, &View::all(), &mut session, Some(&global));
        let g_at = out.find("## Reglas globales").expect("global section");
        let p_at = out.find("## Reglas del proyecto").expect("project section");
        assert!(g_at < p_at, "global first:\n{out}");
        assert!(out.contains("- #g0 regla"), "global ids are marked: {out}");
        assert!(session.was_injected(own));
        assert!(!session.was_injected(dup), "the project rule repeating a global one is skipped");
        assert!(!out.contains("el usuario revisa el diff"));

        // Without a global memory nothing changes.
        let mut m2 = Memory::default();
        let mut c = Claim::rule("Nunca hagas push directo a main", "");
        c.core = true;
        learn(&mut m2, &c, 1.0);
        let plain = handle_session_start(&mut m2, &View::all(), &mut SessionState::new("p"));
        assert!(!plain.contains("globales") && plain.contains("push directo"));
    }

    #[test]
    fn rules_the_agent_already_loads_are_not_injected_twice() {
        let mut m = Memory::default();
        let mut own = Claim::rule("No uses unwrap en el colisionador", "src/core/collider.rs");
        own.core = true;
        let own = learn(&mut m, &own, 1.0)[0].node;
        let mut imported = Claim::rule("Nunca hagas push directo a main", "");
        imported.core = true;
        let imported = learn(&mut m, &imported, 2.0)[0].node;
        m.nodes[imported as usize].key = "AGENTS.md:L7".into();

        let mut session = SessionState::new("n");
        session.native = vec!["AGENTS.md".into(), "GEMINI.md".into()];
        let out = handle_session_start(&mut m, &View::all(), &mut session);
        assert!(session.was_injected(own) && !out.contains("push directo"), "{out}");

        // Claude Code does not load AGENTS.md: there the same memory is injected.
        let mut claude = SessionState::new("c");
        claude.native = vec!["CLAUDE.md".into()];
        assert!(handle_session_start(&mut m, &View::all(), &mut claude).contains("push directo"));
    }

    #[test]
    fn budgets_scale_with_the_memory_and_level_off() {
        // A 1 265-token AGENTS.md gets more CORE than the old fixed 500 asked for in proportion,
        // and a 12 800-token one (three .md files of a real project) more, but never past the ceiling.
        let small = Budgets::for_size(1265);
        let large = Budgets::for_size(12800);
        let huge = Budgets::for_size(200_000);
        assert_eq!((small.core, small.prompt, small.read), (686, 775, 407));
        assert_eq!((large.core, large.prompt, large.read), (1343, 1730, 573));
        assert!(huge.core < CORE_CEIL as usize && huge.core > large.core);
        assert_eq!((small.conversation, large.conversation), (2000, 12800));
        // The vaguer the message, the more memory it gets.
        assert_eq!(small.for_prompt("arréglalo"), 715);
        let long = "el panel de detalle muestra mal el origen del nodo cuando el grafo tiene muchas aristas de causa y contradicción";
        assert!(small.for_prompt(long) < small.prompt * 3 / 5, "{}", small.for_prompt(long));
        // A tiny memory fits whole: below the floors nothing is cut.
        let tiny = Budgets::for_size(120);
        assert_eq!((tiny.core, tiny.prompt, tiny.read), (120, 120, 120));
    }

    #[test]
    fn a_conversation_never_injects_more_than_the_memory_it_replaces() {
        let mut m = Memory::default();
        for i in 0..30 {
            let words: Vec<String> = ["alfa", "beta", "gamma", "delta", "epsilon", "zeta"].iter().map(|w| format!("{w}{i}")).collect();
            learn(&mut m, &Claim::rule(&words.join(" "), &format!("src/m{i}.rs")), 1.0 + i as f64);
        }
        let b = Budgets::of(&m);
        let mut session = SessionState::new("cap");
        let mut total = 0;
        for i in 0..30 {
            total += text::approx_tokens(&read_context(&mut m, &View::all(), &mut session, &format!("src/m{i}.rs")));
        }
        assert!(session.spent as usize <= b.conversation, "{} > {}", session.spent, b.conversation);
        assert!(total > 0);
        let encoded = session.encode();
        assert_eq!(SessionState::decode(&encoded).unwrap().spent, session.spent);
    }

    #[test]
    fn test_proposal_in_neighbor_not_injected() {
        use crate::index::{FileEntry, ProjectIndex};
        use crate::lang_rust::Import;

        let mut m = Memory::default();
        let view = View::all();
        let mut session = SessionState::new("test");

        // Proposed rule anchored at src/cli.rs
        m.add_node(Node {
            id: 0,
            kind: NodeKind::Rule,
            origin: Origin::User,
            flags: flags::PROPOSED,
            path: "src/cli.rs".to_string(),
            symbol: String::new(),
            branch: String::new(),
            commit: String::new(),
            y: 1.0,
            alpha: 4.0,
            beta: 1.0,
            activation: 0.0,
            recalls: 0,
            created_session: 0,
            key: String::new(),
            label: "proposed cli rule".to_string(),
            text: "rule text".to_string(),
            aliases: vec![],
        });

        // Index where src/main.rs imports src/cli.rs
        let mut idx = ProjectIndex::default();
        idx.files.push(FileEntry {
            path: "src/main.rs".to_string(),
            lang: "rust".to_string(),
            size: 10,
            mtime: 10,
            hash: 1,
                    status: "-".to_string(),
            symbols: vec![],
            imports: vec![Import { raw: "use cli;".to_string(), resolved: Some("src/cli.rs".to_string()) }],
            calls: vec![],
        });
        idx.files.push(FileEntry {
            path: "src/cli.rs".to_string(),
            lang: "rust".to_string(),
            size: 10,
            mtime: 10,
            hash: 2,
                    status: "-".to_string(),
            symbols: vec![],
            imports: vec![],
            calls: vec![],
        });

        // Reading src/main.rs: neighbor is src/cli.rs, but proposal must NEVER be injected!
        let out = read_context_with_index(&mut m, &view, &mut session, "src/main.rs", Some(&idx));
        assert!(out.is_empty(), "proposal in neighbor must not be unlocked by reading main.rs");
    }

    #[test]
    fn test_injected_line_carries_neighbor_of_and_without_index_is_same() {
        use crate::index::{FileEntry, ProjectIndex};
        use crate::lang_rust::Import;

        let mut m = Memory::default();
        let view = View::all();
        let mut session1 = SessionState::new("test1");
        let mut session2 = SessionState::new("test2");

        m.add_node(Node {
            id: 0,
            kind: NodeKind::Rule,
            origin: Origin::User,
            flags: 0,
            path: "src/cli.rs".to_string(),
            symbol: String::new(),
            branch: String::new(),
            commit: String::new(),
            y: 1.0,
            alpha: 4.0,
            beta: 1.0,
            activation: 0.0,
            recalls: 0,
            created_session: 0,
            key: String::new(),
            label: "cli rule".to_string(),
            text: "En src/cli.rs los errores salen con 2".to_string(),
            aliases: vec![],
        });

        let mut idx = ProjectIndex::default();
        idx.files.push(FileEntry {
            path: "src/main.rs".to_string(),
            lang: "rust".to_string(),
            size: 10,
            mtime: 10,
            hash: 1,
                    status: "-".to_string(),
            symbols: vec![],
            imports: vec![Import { raw: "use cli;".to_string(), resolved: Some("src/cli.rs".to_string()) }],
            calls: vec![],
        });
        idx.files.push(FileEntry {
            path: "src/cli.rs".to_string(),
            lang: "rust".to_string(),
            size: 10,
            mtime: 10,
            hash: 2,
                    status: "-".to_string(),
            symbols: vec![],
            imports: vec![],
            calls: vec![],
        });

        // 1. Without index: reading src/main.rs brings nothing for cli.rs
        let out_no_idx = read_context_with_index(&mut m, &view, &mut session1, "src/main.rs", None);
        assert!(out_no_idx.is_empty());

        // 2. With index: reading src/main.rs brings the neighbor rule and carries "vecino de"
        let out_with_idx = read_context_with_index(&mut m, &view, &mut session2, "src/main.rs", Some(&idx));
        assert!(out_with_idx.contains("vecino de src/main.rs"), "line carries 'vecino de src/main.rs'");
    }

    #[test]
    fn writing_a_file_without_reading_it_injects_its_rules_once_and_reading_after_does_not_repeat() {
        let mut m = Memory::default();
        let file_id = learn(&mut m, &Claim::rule("En src/id.rs los ids son base36 de 4 caracteres", "src/id.rs"), 100.0)[0].node;
        let dir_id = learn(&mut m, &Claim::rule("Todo src/ usa solo la biblioteca estándar", "src"), 101.0)[0].node;
        m.nodes[file_id as usize].activation = 0.4;
        m.nodes[dir_id as usize].activation = 0.4;

        let view = View::all();
        let mut session = SessionState::new("edit-first");

        // Writing `src/id.rs` without having read it refreshes activation and injects its rules once.
        let e_out = edit_context(&mut m, &view, &mut session, "src/id.rs");
        assert!(e_out.starts_with("## Memoria relevante (src/id.rs, que acabas de escribir)"), "{e_out}");
        assert!(e_out.contains("base36 de 4 caracteres"), "{e_out}");
        assert!(e_out.contains("solo la biblioteca estándar"), "{e_out}");
        assert_eq!(m.nodes[file_id as usize].activation, 1.0);
        assert_eq!(m.nodes[dir_id as usize].activation, 1.0);
        assert!(session.was_injected(file_id) && session.was_injected(dir_id));

        // Writing it again or reading it afterwards does not repeat the already-injected rules.
        assert!(edit_context(&mut m, &view, &mut session, "src/id.rs").is_empty());
        assert!(read_context(&mut m, &view, &mut session, "src/id.rs").is_empty());
        assert!(handle_read(&mut m, &view, &mut session, "src/id.rs").is_empty());
    }

    #[test]
    fn test_superseded_not_injected_in_any_channel_and_reappears_when_substitute_denied() {
        use crate::index::{FileEntry, ProjectIndex};
        use crate::lang_rust::Import;
        let mut m = Memory::default();
        let view = View::all();

        // 1. CORE rule: old is CORE, new replaces it
        let mut old_core_claim = Claim::rule("Presupuestos fijos 500 800", "");
        old_core_claim.core = true;
        let old_core = learn(&mut m, &old_core_claim, 100.0)[0].node;

        let mut new_core_claim = Claim::rule("Presupuestos dinámicos según S", "");
        new_core_claim.core = true;
        new_core_claim.supersedes = Some(old_core);
        let ev_core = learn(&mut m, &new_core_claim, 101.0);
        let new_core = ev_core.iter().find(|e| e.action == "nuevo").unwrap().node;

        // 2. File / neighbor rule: old_file is for src/cli.rs, replaced by new_file
        let old_file = learn(&mut m, &Claim::rule("CLI antiguo flag viejo", "src/cli.rs"), 102.0)[0].node;
        let mut new_file_claim = Claim::rule("CLI nuevo flag nuevo", "src/cli.rs");
        new_file_claim.supersedes = Some(old_file);
        let ev_file = learn(&mut m, &new_file_claim, 103.0);
        let new_file = ev_file.iter().find(|e| e.action == "nuevo").unwrap().node;

        // Code index linking src/main.rs -> src/cli.rs as neighbor
        let mut idx = ProjectIndex::default();
        idx.files.push(FileEntry {
            path: "src/main.rs".to_string(),
            lang: "rust".to_string(),
            size: 10,
            mtime: 10,
            hash: 1,
            status: "-".to_string(),
            symbols: vec![],
            imports: vec![Import { raw: "use cli;".to_string(), resolved: Some("src/cli.rs".to_string()) }],
            calls: vec![],
        });
        idx.files.push(FileEntry {
            path: "src/cli.rs".to_string(),
            lang: "rust".to_string(),
            size: 10,
            mtime: 10,
            hash: 2,
            status: "-".to_string(),
            symbols: vec![],
            imports: vec![],
            calls: vec![],
        });

        // Test Channel 0: Core context
        let mut s_core = SessionState::new("s-core");
        let core_out = core_context(&mut m, &view, &mut s_core, None);
        assert!(!core_out.contains("Presupuestos fijos 500 800"), "old core must not be injected");
        assert!(core_out.contains("Presupuestos dinámicos"), "new core must be injected");

        // Test Channel 1 & 3: Prompt / message
        let mut s_msg = SessionState::new("s-msg");
        let msg_out = handle_prompt(&mut m, &view, &mut s_msg, "flag viejo o nuevo en CLI", vec![]);
        assert!(!msg_out.contains("CLI antiguo flag viejo"), "old rule must not be injected in prompt");
        assert!(msg_out.contains("CLI nuevo flag nuevo"), "new rule must be injected in prompt");

        // Test Channel 2: Read file directly
        let mut s_read = SessionState::new("s-read");
        let read_out = read_context(&mut m, &view, &mut s_read, "src/cli.rs");
        assert!(!read_out.contains("CLI antiguo flag viejo"), "old rule must not be injected in read");
        assert!(read_out.contains("CLI nuevo flag nuevo"), "new rule must be injected in read");

        // Test Neighbor channel: Reading neighbor src/main.rs pulls src/cli.rs
        let mut s_neigh = SessionState::new("s-neigh");
        let neigh_out = read_context_with_index(&mut m, &view, &mut s_neigh, "src/main.rs", Some(&idx));
        assert!(!neigh_out.contains("CLI antiguo flag viejo"), "old rule must not be injected as neighbor");
        assert!(neigh_out.contains("CLI nuevo flag nuevo"), "new rule must be injected as neighbor");

        // Now deny the replacement until chi <= 0
        for _ in 0..10 {
            learn::feedback(&mut m, new_core, -1);
            learn::feedback(&mut m, new_file, -1);
        }
        let scores = score::score_all(&m, &view);
        assert!(scores[new_core as usize].chi <= 0.0);
        assert!(scores[new_file as usize].chi <= 0.0);
assert!(!scores[old_core as usize].superseded, "old core is no longer superseded");
        assert!(!scores[old_file as usize].superseded, "old file is no longer superseded");

        // Reappears in core and read!
        let mut s_core2 = SessionState::new("s-core2");
        let core_reappear = core_context(&mut m, &view, &mut s_core2, None);
        assert!(core_reappear.contains("Presupuestos fijos 500 800"), "old core reappears when new is denied");

        let mut s_read2 = SessionState::new("s-read2");
        let read_reappear = read_context(&mut m, &view, &mut s_read2, "src/cli.rs");
        assert!(read_reappear.contains("CLI antiguo flag viejo"), "old rule reappears when new is denied");
    }

    #[test]
    fn test_anchored_with_supersedes_edge_continues_to_be_injected() {
        let mut m = Memory::default();
        let view = View::all();
        let mut fact_claim = Claim::rule("Fact proven by repo", "src/core.rs");
        fact_claim.kind = NodeKind::Fact;
        fact_claim.origin = Origin::Repo;
        let fact = learn(&mut m, &fact_claim, 100.0)[0].node;

        let repl = learn(&mut m, &Claim::rule("User tries to replace fact", "src/core.rs"), 101.0)[0].node;
        m.upsert_edge(repl, fact, EdgeKind::Supersedes, 1.0, 1.0);

        let mut session = SessionState::new("anchored-test");
        let out = read_context(&mut m, &view, &mut session, "src/core.rs");
        assert!(out.contains("Fact proven by repo"), "anchored fact must continue to be injected");
    }

    #[test]
    fn test_render_node_terminates_on_cycle_of_3() {
        let mut m = Memory::default();
        let n1 = learn(&mut m, &Claim::rule("Primera regla de prueba ciclo", "src"), 100.0)[0].node;
        let n2 = learn(&mut m, &Claim::rule("Segunda regla de prueba ciclo", "src"), 101.0)[0].node;
        let n3 = learn(&mut m, &Claim::rule("Tercera regla de prueba ciclo", "src"), 102.0)[0].node;
        // Decision with cause n1
        let mut d = Claim::rule("Decision relying on n1 ciclo", "src");
        d.kind = NodeKind::Decision;
        let dec = learn(&mut m, &d, 103.0)[0].node;
        m.upsert_edge(n1, dec, EdgeKind::Causes, 1.0, 1.0);

        // Create cycle of 3 manually: n1 -> n2 -> n3 -> n1
        m.upsert_edge(n2, n1, EdgeKind::Supersedes, 1.0, 1.0);
        m.upsert_edge(n3, n2, EdgeKind::Supersedes, 1.0, 1.0);
        m.upsert_edge(n1, n3, EdgeKind::Supersedes, 1.0, 1.0);

        let scores = score::score_all(&m, &View::all());
        // Must terminate safely without infinite loop
        let line = render_node(&m, &scores, &m.nodes[dec as usize]);
        assert!(line.contains("Decision relying on n1"));
    }

    #[test]
    fn test_record_and_load_global_recalls() {
        let dir = std::env::temp_dir().join(format!("cthg-recalls-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);

        let ids = vec![0, 1, 5];
        record_global_recalls(&dir, &ids);
        let m1 = load_global_recalls(&dir);
        assert_eq!(m1.get(&0), Some(&1));
        assert_eq!(m1.get(&1), Some(&1));
        assert_eq!(m1.get(&5), Some(&1));
        assert_eq!(m1.get(&2), None);

        // Record again with overlap
        record_global_recalls(&dir, &[1, 2]);
        let m2 = load_global_recalls(&dir);
        assert_eq!(m2.get(&0), Some(&1));
        assert_eq!(m2.get(&1), Some(&2));
        assert_eq!(m2.get(&2), Some(&1));
        assert_eq!(m2.get(&5), Some(&1));

        let _ = fs::remove_dir_all(&dir);
    }
}