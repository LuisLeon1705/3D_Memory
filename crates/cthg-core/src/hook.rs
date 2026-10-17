//! Claude Code lifecycle hooks (`cthg hook <session-start|prompt|read|edit>`).
//!
//! Rules:
//! - Never block or fail out loud: on any error, exit with code 0 and print nothing.
//! - Per-session deduplication in `<repo>/.cthg/sesion/<session_id>.json`: a memory node injected
//!   once in a session is not injected again in that same session.
//! - Token budgets: `session-start` (core) ≤ 500, `prompt` ≤ 800, `read` (per file) ≤ 300.
//! - `PostToolUse` (`read`) wraps injected context in `hookSpecificOutput.additionalContext` JSON.

use super::learn;
use super::model::*;
use super::retrieve::{self, Query};
use super::score::{self, certainty_tag, Score, View};
use super::spatial::{self, Target};
use super::text;
use std::fs;
use std::path::{Path, PathBuf};

pub const BUDGET_CORE: usize = 500;
pub const BUDGET_PROMPT: usize = 800;
pub const BUDGET_READ: usize = 300;

/// Parsed fields from Claude Code's hook `stdin` JSON payload.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookInput {
    pub session_id: String,
    pub cwd: String,
    pub prompt: String,
    pub file_path: String,
}

impl HookInput {
    /// Extracts `session_id`, `cwd`, `prompt`, and `tool_input.file_path` from a JSON payload.
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
            "{{\"session_id\":\"{}\",\"injected\":[{}],\"offset\":{},\"turn\":{},\"started\":{}}}\n",
            escape_json(&self.session_id),
            nums.join(","),
            self.offset,
            self.turn,
            self.started
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

/// Renders one memory node line: `- #id tipo (ruta::simbolo): texto ← causa`.
pub fn render_node(m: &Memory, scores: &[Score], n: &Node) -> String {
    let mut s = format!("- #{} {}", n.id, n.kind.name());
    if !n.path.is_empty() {
        s.push_str(&format!(" ({}", n.path));
        if !n.symbol.is_empty() {
            s.push_str(&format!("::{}", n.symbol));
        }
        s.push(')');
    }
    s.push_str(&format!(": {}", text::clip(injected_text(n), 240)));
    let cause = m
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Causes && e.to == n.id && scores[e.from as usize].visible)
        .max_by(|a, b| scores[a.from as usize].chi.partial_cmp(&scores[b.from as usize].chi).unwrap_or(std::cmp::Ordering::Equal))
        .map(|e| &m.nodes[e.from as usize]);
    if let Some(c) = cause {
        s.push_str(&format!(" ← {}", text::clip(&c.label, 80)));
    }
    s
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

/// Formats `ids` grouped by χ certainty band under `heading`.
pub fn format_bands(heading: &str, m: &Memory, scores: &[Score], ids: &[u32]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let mut out = String::from(heading);
    let bands = ["verificado", "probable", "sin evidencia", "dudoso", "descartado"];
    for band in bands {
        let lines: Vec<String> = ids
            .iter()
            .filter(|&&id| certainty_tag(scores[id as usize].chi) == band)
            .map(|&id| render_node(m, scores, &m.nodes[id as usize]))
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

/// Runs `session-start`: one forgetting step, then injects `CORE` nodes within `BUDGET_CORE`.
pub fn handle_session_start(m: &mut Memory, view: &View, session: &mut SessionState) -> String {
    handle_session_start_layered(m, view, session, None)
}

/// The user's global `CORE` (rules for every project), within `budget`. Read-only: the global
/// memory is shared by every project, so a session never changes it. Returns the text and the
/// tokens it used. Global ids print as `#g3` so they are not confused with the project's.
pub fn global_core(global: &Memory, budget: usize) -> (String, usize, Vec<Vec<String>>) {
    let scores = score::score_all(global, &View::all());
    let all = retrieve::core(global, &scores);
    let ids = retrieve::fit_budget(global, &all, budget, |n| render_node(global, &scores, n));
    if ids.is_empty() {
        return (String::new(), 0, Vec::new());
    }
    let used: usize = ids.iter().map(|&id| text::approx_tokens(&render_node(global, &scores, &global.nodes[id as usize]))).sum();
    let mut out = format_bands("## Reglas globales (siempre, en todos los proyectos)", global, &scores, &ids).replace("\n- #", "\n- #g");
    if ids.len() < all.len() {
        out.push_str(&format!("\n({} reglas globales no caben aquí; `cthg core --global` las muestra todas)", all.len() - ids.len()));
    }
    let toks = ids.iter().map(|&id| text::tokens(&global.nodes[id as usize].text)).collect();
    (out, used, toks)
}

/// A project rule that says what a global rule already said is not injected twice.
pub const DUPLICATE_OF_GLOBAL: f32 = 0.5;

/// Session start with the global layer first: global `CORE`, then the project's `CORE` in the
/// budget left, skipping project rules that repeat a global one.
pub fn handle_session_start_layered(m: &mut Memory, view: &View, session: &mut SessionState, global: Option<&Memory>) -> String {
    session.injected.clear();
    learn::begin_session(m);
    session.started = true;
    core_context(m, view, session, global)
}

/// The `CORE` text (global layer, then the project's) without starting a session: for agents
/// whose injected context only lasts one turn and must be given again on the next one.
pub fn core_context(m: &mut Memory, view: &View, session: &mut SessionState, global: Option<&Memory>) -> String {
    let (global_out, used, global_toks) = global.map(|g| global_core(g, BUDGET_CORE)).unwrap_or_default();
    let scores = score::score_all(m, view);
    let candidates: Vec<u32> = retrieve::core(m, &scores)
        .into_iter()
        .filter(|&id| !session.was_injected(id) && !session.is_native(&m.nodes[id as usize]))
        .filter(|&id| {
            let toks = text::tokens(&m.nodes[id as usize].text);
            !global_toks.iter().any(|g| text::similarity(&toks, g) >= DUPLICATE_OF_GLOBAL)
        })
        .collect();
    let ids = retrieve::fit_budget(m, &candidates, BUDGET_CORE.saturating_sub(used), |n| render_node(m, &scores, n));
    if ids.is_empty() {
        return global_out;
    }
    session.record(&ids);
    for &id in &ids {
        if let Some(n) = m.node_mut(id) {
            n.activation = 1.0;
            n.recalls += 1;
        }
    }
    let mut out = format_bands("## Reglas del proyecto (siempre)", m, &scores, &ids);
    // Never drop core rules silently: the agent should know some are missing.
    let missing = candidates.len() - ids.len();
    if missing > 0 {
        out.push_str(&format!("\n({missing} reglas del núcleo no caben aquí; `cthg core` las muestra todas)"));
    }
    if global_out.is_empty() {
        out
    } else {
        format!("{global_out}\n\n{out}")
    }
}

/// Runs `prompt`: fuses lexical and `git status`/`git diff` spatial channels within `BUDGET_PROMPT`.
pub fn handle_prompt(
    m: &mut Memory,
    view: &View,
    session: &mut SessionState,
    prompt: &str,
    git_targets: Vec<Target>,
) -> String {
    let scores = score::score_all(m, view);
    let q = Query { text: prompt.to_string(), targets: git_targets };
    let candidates: Vec<u32> = retrieve::recall(m, &scores, &q, 40, -1.0)
        .into_iter()
        .map(|r| r.id)
        .filter(|&id| !session.was_injected(id) && !session.is_native(&m.nodes[id as usize]))
        .collect();
    let ids = retrieve::fit_budget(m, &candidates, BUDGET_PROMPT, |n| render_node(m, &scores, n));
    if ids.is_empty() {
        return String::new();
    }
    session.record(&ids);
    learn::co_recalled(m, &ids);
    format_bands("## Memoria relevante", m, &scores, &ids)
}

/// Runs `read`: just-in-time spatial recall for `rel_path` within `BUDGET_READ`, wrapped in
/// `PostToolUse` `hookSpecificOutput.additionalContext` JSON.
pub fn handle_read(m: &mut Memory, view: &View, session: &mut SessionState, rel_path: &str) -> String {
    let context = read_context(m, view, session, rel_path);
    if context.is_empty() {
        return context;
    }
    format_post_tool_use(&context)
}

/// The plain context `read` injects (what `handle_read` wraps in JSON); `eval` counts its tokens.
pub fn read_context(m: &mut Memory, view: &View, session: &mut SessionState, rel_path: &str) -> String {
    let rel_path = spatial::normalize(rel_path);
    if rel_path.is_empty() {
        return String::new();
    }
    let scores = score::score_all(m, view);
    let q = Query {
        text: String::new(),
        targets: vec![Target::new(&rel_path, 1.0)],
    };
    let candidates: Vec<u32> = retrieve::recall(m, &scores, &q, 40, -1.0)
        .into_iter()
        .map(|r| r.id)
        .filter(|&id| !session.was_injected(id) && !session.is_native(&m.nodes[id as usize]))
        .collect();
    let ids = retrieve::fit_budget(m, &candidates, BUDGET_READ, |n| render_node(m, &scores, n));
    if ids.is_empty() {
        return String::new();
    }
    session.record(&ids);
    learn::co_recalled(m, &ids);
    let heading = format!("## Memoria relevante ({rel_path})");
    format_bands(&heading, m, &scores, &ids)
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
}

