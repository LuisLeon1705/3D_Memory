//! Measurement (`cthg eval`): how many tokens each configuration injects and which relevant
//! rules it misses, compared with loading the whole `CLAUDE.md`.
//!
//! Everything runs on a fresh in-memory import with a fixed clock, so results are deterministic
//! and the project's own `.cthg/` is never touched. Each relevant rule that is not injected gets
//! exactly one diagnosis, the first that applies:
//! no importada → fusionada al importar → χ bajo → fuera de presupuesto → sin ruta y no CORE →
//! ruta no coincide / sin coincidencia léxica.

use super::hook::{self, SessionState};
use super::import;
use super::model::*;
use super::retrieve::{self, Query};
use super::score::{self, Score, View};
use super::spatial::{self, Target};
use super::text;
use std::collections::BTreeMap;

/// Fixed clock for imports, so two runs give the same numbers.
const NOW: f64 = 1000.0;
/// `git` paths are modified files: same weight as in `git status`.
const GIT_WEIGHT: f32 = 0.8;

// ---------------------------------------------------------------------------------------------
// Case file

/// One evaluation case from `casos.md`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Case {
    pub name: String,
    /// Line of its `## caso:` heading, for messages.
    pub line: usize,
    pub prompt: String,
    /// Modified paths (channel 1).
    pub git: Vec<String>,
    /// Files the agent would read (channel 2).
    pub read: Vec<String>,
    /// `(line in CLAUDE.md, critical)`.
    pub relevant: Vec<(usize, bool)>,
    /// Confirmed `CORE` lines for this case; `None` = the file-level list, or import's proposals.
    pub core: Option<Vec<usize>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaseFile {
    /// `core:` before the first case: confirmed `CORE` lines for the whole repo.
    pub core: Option<Vec<usize>>,
    pub cases: Vec<Case>,
}

/// Parses `casos.md`. Errors are `línea N: motivo`.
///
/// ```text
/// core: L3, L5                      (optional, before the first case)
/// ## caso: arreglar el colisionador
/// prompt: arréglalo
/// git: src/core/collider.rs
/// lee: src/core/collider.rs, src/core/mod.rs
/// relevantes: L12, L18!, L30        (`!` = critical; empty = not labelled yet)
/// core: L3, L5                      (optional override)
/// ```
pub fn parse_cases(md: &str) -> Result<CaseFile, String> {
    let mut file = CaseFile::default();
    for (i, raw) in md.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if let Some(name) = line.strip_prefix("## caso:") {
            let name = name.trim();
            if name.is_empty() {
                return Err(format!("línea {n}: el caso no tiene nombre"));
            }
            file.cases.push(Case { name: name.to_string(), line: n, ..Case::default() });
            continue;
        }
        if line.is_empty() || line.starts_with('#') || line.starts_with('>') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            if file.cases.is_empty() {
                continue; // free text before the first case
            }
            return Err(format!("línea {n}: se esperaba `clave: valor`"));
        };
        let key = key.trim();
        let value = value.trim();
        let Some(case) = file.cases.last_mut() else {
            match key {
                "core" => file.core = Some(parse_lines(value, n)?.into_iter().map(|(l, _)| l).collect()),
                _ => continue, // free text before the first case
            }
            continue;
        };
        match key {
            "prompt" => case.prompt = value.to_string(),
            "git" => case.git = parse_paths(value),
            "lee" => case.read = parse_paths(value),
            "relevantes" => case.relevant = parse_lines(value, n)?,
            "core" => case.core = Some(parse_lines(value, n)?.into_iter().map(|(l, _)| l).collect()),
            other => return Err(format!("línea {n}: clave desconocida `{other}` (prompt, git, lee, relevantes, core)")),
        }
    }
    if file.cases.is_empty() {
        return Err("no hay ningún `## caso:`".into());
    }
    Ok(file)
}

/// Drops a trailing ` # comentario` (lists only: prompts may contain `#`).
fn strip_comment(v: &str) -> &str {
    if v.starts_with('#') {
        return "";
    }
    v.split(" #").next().unwrap_or(v).trim()
}

fn parse_paths(v: &str) -> Vec<String> {
    strip_comment(v)
        .split(',')
        .map(spatial::normalize)
        .filter(|p| !p.is_empty())
        .collect()
}

/// `L12, L18!, 30` → `[(12, false), (18, true), (30, false)]`.
fn parse_lines(v: &str, n: usize) -> Result<Vec<(usize, bool)>, String> {
    let mut out = Vec::new();
    for item in strip_comment(v).split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let critical = item.ends_with('!');
        let num = item.trim_end_matches('!').trim_start_matches(['L', 'l']);
        let line: usize = num.parse().map_err(|_| format!("línea {n}: `{item}` no es una línea (usa L12 o L12!)"))?;
        if line == 0 {
            return Err(format!("línea {n}: las líneas empiezan en L1"));
        }
        if !out.iter().any(|&(l, _)| l == line) {
            out.push((line, critical));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Configurations and results

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Config {
    /// The whole file, every session.
    Completo,
    /// Per-folder CLAUDE.md files done perfectly: pathless rules + rules above touched/read files, no budget.
    AnidadoIdeal,
    /// Channel 0 + 3.
    SoloLexico,
    /// Channel 0 + 1.
    Espacial,
    /// Channel 0 + 1 + 2.
    EspacialLectura,
    /// Channels 0–3, with per-session deduplication, as the hooks run.
    Todo,
}

impl Config {
    pub const ALL: [Config; 6] = [
        Config::Completo,
        Config::AnidadoIdeal,
        Config::SoloLexico,
        Config::Espacial,
        Config::EspacialLectura,
        Config::Todo,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Config::Completo => "completo",
            Config::AnidadoIdeal => "anidado ideal",
            Config::SoloLexico => "solo léxico",
            Config::Espacial => "espacial",
            Config::EspacialLectura => "espacial + lectura",
            Config::Todo => "todo",
        }
    }

    fn lexical(self) -> bool {
        matches!(self, Config::SoloLexico | Config::Todo)
    }

    fn spatial(self) -> bool {
        matches!(self, Config::AnidadoIdeal | Config::Espacial | Config::EspacialLectura | Config::Todo)
    }

    fn reads(self) -> bool {
        matches!(self, Config::AnidadoIdeal | Config::EspacialLectura | Config::Todo)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissReason {
    NotImported,
    /// Merged at import time into the node that came from this line.
    Merged(usize),
    LowChi,
    OverBudget,
    PathlessNotCore,
    PathMismatch,
    NoLexicalMatch,
}

impl MissReason {
    pub fn describe(&self) -> String {
        match self {
            MissReason::NotImported => "no importada".into(),
            MissReason::Merged(l) => format!("fusionada al importar (con L{l})"),
            MissReason::LowChi => "χ bajo".into(),
            MissReason::OverBudget => "fuera de presupuesto".into(),
            MissReason::PathlessNotCore => "sin ruta y no CORE".into(),
            MissReason::PathMismatch => "ruta no coincide".into(),
            MissReason::NoLexicalMatch => "sin coincidencia léxica".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Miss {
    pub line: usize,
    pub critical: bool,
    pub reason: MissReason,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub config: Config,
    pub tokens: usize,
    /// Rules injected (for `completo`: every imported block).
    pub injected: usize,
    pub recall: f32,
    pub precision: f32,
    pub misses: Vec<Miss>,
}

impl Outcome {
    pub fn critical_missed(&self) -> usize {
        self.misses.iter().filter(|m| m.critical).count()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaseResult {
    pub name: String,
    pub outcomes: Vec<Outcome>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EvalReport {
    pub source: String,
    /// Blocks import turned into nodes or merged.
    pub blocks: usize,
    pub merged: usize,
    /// "confirmado en casos.md" or "propuesto por import".
    pub core_mode: &'static str,
    pub core_count: usize,
    /// `CORE` lines in `casos.md` that are not an imported block.
    pub core_unknown: Vec<usize>,
    pub cases: Vec<CaseResult>,
    /// Cases with no `relevantes:` yet: listed, not measured.
    pub unlabelled: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// Running

/// Imported memory for one `CORE` choice, with the line → node maps.
struct Imported {
    m: Memory,
    by_line: BTreeMap<usize, u32>,
    merged: BTreeMap<usize, u32>,
    blocks: usize,
    core_unknown: Vec<usize>,
}

fn line_of(n: &Node) -> Option<usize> {
    n.key.rsplit_once(":L").and_then(|(_, l)| l.parse().ok())
}

fn import_for(source: &str, markdown: &str, core: Option<&[usize]>, resolve: &dyn Fn(&str) -> Option<String>) -> Imported {
    let mut m = Memory::default();
    let report = import::import_markdown_named(&mut m, source, markdown, "", "", NOW, core.is_none(), resolve);
    let by_line: BTreeMap<usize, u32> = m.nodes.iter().filter_map(|n| line_of(n).map(|l| (l, n.id))).collect();
    let merged: BTreeMap<usize, u32> = report.merged.iter().copied().collect();
    let mut core_unknown = Vec::new();
    if let Some(lines) = core {
        for n in m.nodes.iter_mut() {
            n.flags &= !flags::CORE;
        }
        for &l in lines {
            match by_line.get(&l) {
                Some(&id) => m.nodes[id as usize].flags |= flags::CORE,
                None => core_unknown.push(l),
            }
        }
    }
    Imported { blocks: by_line.len() + merged.len(), m, by_line, merged, core_unknown }
}

fn git_targets(case: &Case) -> Vec<Target> {
    case.git.iter().map(|p| Target::new(p, GIT_WEIGHT)).collect()
}

/// Runs one channel configuration the way the hooks would. Returns (tokens, injected node ids).
fn simulate(base: &Memory, case: &Case, cfg: Config) -> (usize, Vec<u32>) {
    let mut m = base.clone();
    let view = View::all();
    if cfg == Config::AnidadoIdeal {
        let scores = score::score_all(&m, &view);
        let mut touched: Vec<String> = case.git.clone();
        touched.extend(case.read.iter().cloned());
        let ids: Vec<u32> = m
            .nodes
            .iter()
            .filter(|n| scores[n.id as usize].visible)
            .filter(|n| n.path.is_empty() || touched.iter().any(|t| spatial::is_ancestor_or_self(&n.path, t)))
            .map(|n| n.id)
            .collect();
        let tokens = ids.iter().map(|&id| text::approx_tokens(&hook::render_node(&m, &scores, &m.nodes[id as usize]))).sum();
        return (tokens, ids);
    }
    let mut session = SessionState::new("eval");
    let mut out = hook::handle_session_start(&mut m, &view, &mut session);
    let prompt = if cfg.lexical() { case.prompt.as_str() } else { "" };
    let targets = if cfg.spatial() { git_targets(case) } else { vec![] };
    out.push_str(&hook::handle_prompt(&mut m, &view, &mut session, prompt, targets));
    if cfg.reads() {
        for p in &case.read {
            out.push_str(&hook::read_context(&mut m, &view, &mut session, p));
        }
    }
    (text::approx_tokens(&out), session.injected)
}

/// Why a relevant node was not injected under `cfg`: the first cause that applies.
fn diagnose(m: &Memory, scores: &[Score], case: &Case, cfg: Config, id: u32) -> MissReason {
    let n = &m.nodes[id as usize];
    let s = scores[id as usize];
    if !s.visible || s.chi <= -0.25 {
        return MissReason::LowChi;
    }
    if cfg != Config::AnidadoIdeal {
        // Would it have come back with no budget at all?
        if n.has(flags::CORE) {
            return MissReason::OverBudget;
        }
        let mut found = false;
        let prompt = if cfg.lexical() { case.prompt.clone() } else { String::new() };
        let targets = if cfg.spatial() { git_targets(case) } else { vec![] };
        let q = Query { text: prompt, targets };
        found |= retrieve::recall(m, scores, &q, usize::MAX, -1.0).iter().any(|r| r.id == id);
        if cfg.reads() {
            for p in &case.read {
                let q = Query { text: String::new(), targets: vec![Target::new(p, 1.0)] };
                found |= retrieve::recall(m, scores, &q, usize::MAX, -1.0).iter().any(|r| r.id == id);
            }
        }
        if found {
            return MissReason::OverBudget;
        }
    }
    // "Wrong path" only means something if this configuration had paths to compare with.
    let had_paths = cfg.spatial() && (!case.git.is_empty() || (cfg.reads() && !case.read.is_empty()));
    if n.path.is_empty() {
        MissReason::PathlessNotCore
    } else if had_paths || cfg == Config::AnidadoIdeal || !cfg.lexical() {
        MissReason::PathMismatch
    } else {
        MissReason::NoLexicalMatch
    }
}

fn evaluate_case(imp: &Imported, case: &Case, items: usize, full_tokens: usize) -> CaseResult {
    let scores = score::score_all(&imp.m, &View::all());
    let mut outcomes = Vec::new();
    for cfg in Config::ALL {
        if cfg == Config::Completo {
            let relevant = case.relevant.len() as f32;
            outcomes.push(Outcome {
                config: cfg,
                tokens: full_tokens,
                injected: items,
                recall: 1.0,
                precision: if items == 0 { 0.0 } else { relevant / items as f32 },
                misses: vec![],
            });
            continue;
        }
        let (tokens, injected) = simulate(&imp.m, case, cfg);
        let mut misses = Vec::new();
        let mut hits = 0usize;
        for &(line, critical) in &case.relevant {
            let reason = match imp.by_line.get(&line) {
                Some(&id) if injected.contains(&id) => {
                    hits += 1;
                    continue;
                }
                Some(&id) => diagnose(&imp.m, &scores, case, cfg, id),
                None => match imp.merged.get(&line) {
                    Some(&into) => MissReason::Merged(line_of(&imp.m.nodes[into as usize]).unwrap_or(0)),
                    None => MissReason::NotImported,
                },
            };
            misses.push(Miss { line, critical, reason });
        }
        let relevant = case.relevant.len().max(1) as f32;
        outcomes.push(Outcome {
            config: cfg,
            tokens,
            injected: injected.len(),
            recall: hits as f32 / relevant,
            precision: if injected.is_empty() { 0.0 } else { hits as f32 / injected.len() as f32 },
            misses,
        });
    }
    CaseResult { name: case.name.clone(), outcomes }
}

/// Evaluates `markdown` (whose lines `casos.md` refers to) against every labelled case.
/// `resolve` answers which repo path a mention names (see `import::RepoFiles`).
/// `auto_core` ignores the `core:` lines and uses import's own proposal, to compare the two.
pub fn run(source: &str, markdown: &str, file: &CaseFile, resolve: &dyn Fn(&str) -> Option<String>, auto_core: bool) -> EvalReport {
    let full_tokens = text::approx_tokens(markdown);
    let mut report = EvalReport {
        source: source.to_string(),
        core_mode: if !auto_core && (file.core.is_some() || file.cases.iter().any(|c| c.core.is_some())) {
            "confirmado en casos.md"
        } else {
            "propuesto por import"
        },
        ..EvalReport::default()
    };
    let mut cache: Vec<(Option<Vec<usize>>, Imported)> = Vec::new();
    for case in &file.cases {
        if case.relevant.is_empty() {
            report.unlabelled.push(case.name.clone());
            continue;
        }
        let core = if auto_core { None } else { case.core.clone().or_else(|| file.core.clone()) };
        if !cache.iter().any(|(c, _)| *c == core) {
            cache.push((core.clone(), import_for(source, markdown, core.as_deref(), resolve)));
        }
        let imp = &cache.iter().find(|(c, _)| *c == core).expect("just inserted").1;
        report.blocks = imp.blocks;
        report.merged = imp.merged.len();
        report.core_count = imp.m.nodes.iter().filter(|n| n.has(flags::CORE)).count();
        for &l in &imp.core_unknown {
            if !report.core_unknown.contains(&l) {
                report.core_unknown.push(l);
            }
        }
        report.cases.push(evaluate_case(imp, case, imp.blocks, full_tokens));
    }
    report
}

// ---------------------------------------------------------------------------------------------
// Report

#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub config: Config,
    pub tokens: f32,
    pub saving: f32,
    pub recall: f32,
    pub precision: f32,
    pub critical_missed: usize,
}

pub fn summarize(r: &EvalReport) -> Vec<Summary> {
    let n = r.cases.len().max(1) as f32;
    let full = r
        .cases
        .iter()
        .flat_map(|c| c.outcomes.iter())
        .find(|o| o.config == Config::Completo)
        .map(|o| o.tokens as f32)
        .unwrap_or(0.0);
    Config::ALL
        .iter()
        .map(|&cfg| {
            let os: Vec<&Outcome> = r.cases.iter().filter_map(|c| c.outcomes.iter().find(|o| o.config == cfg)).collect();
            let tokens = os.iter().map(|o| o.tokens as f32).sum::<f32>() / n;
            Summary {
                config: cfg,
                tokens,
                saving: if full > 0.0 { 1.0 - tokens / full } else { 0.0 },
                recall: os.iter().map(|o| o.recall).sum::<f32>() / n,
                precision: os.iter().map(|o| o.precision).sum::<f32>() / n,
                critical_missed: os.iter().map(|o| o.critical_missed()).sum(),
            }
        })
        .collect()
}

fn pct(x: f32) -> String {
    format!("{:.0} %", x * 100.0)
}

fn mark(ok: bool) -> &'static str {
    if ok {
        "✔"
    } else {
        "✘"
    }
}

/// The whole report as Markdown (printed by the CLI and written to `resultado.md`).
pub fn render_markdown(r: &EvalReport, title: &str) -> String {
    let mut s = format!("# Resultado de `cthg eval` — {title}\n\n");
    s.push_str(&format!(
        "{}: {} bloques importados ({} fusionados con otro) · CORE {}: {} reglas · {} casos medidos\n",
        r.source,
        r.blocks,
        r.merged,
        r.core_mode,
        r.core_count,
        r.cases.len()
    ));
    if !r.unlabelled.is_empty() {
        s.push_str(&format!("\nSin etiquetar (no cuentan): {}\n", r.unlabelled.join(", ")));
    }
    if !r.core_unknown.is_empty() {
        let ls: Vec<String> = r.core_unknown.iter().map(|l| format!("L{l}")).collect();
        s.push_str(&format!("\nAviso: `core:` cita líneas que no son un bloque importado: {}\n", ls.join(", ")));
    }
    let sums = summarize(r);
    s.push_str("\n| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |\n");
    s.push_str("|---|---:|---:|---:|---:|---:|\n");
    for x in &sums {
        s.push_str(&format!(
            "| {} | {:.0} | {} | {} | {} | {} |\n",
            x.config.name(),
            x.tokens,
            pct(x.saving),
            pct(x.recall),
            pct(x.precision),
            x.critical_missed
        ));
    }
    if let Some(t) = sums.iter().find(|x| x.config == Config::Todo) {
        s.push_str(&format!(
            "\nCriterio (`todo`): ahorro ≥ 70 % {} · críticas perdidas = 0 {} · recall ≥ 95 % {}\n",
            mark(t.saving >= 0.70),
            mark(t.critical_missed == 0),
            mark(t.recall >= 0.95)
        ));
    }

    s.push_str("\n## Por caso (`todo`)\n\n| Caso | Tokens | Recall | Críticas perdidas |\n|---|---:|---:|---:|\n");
    for c in &r.cases {
        if let Some(o) = c.outcomes.iter().find(|o| o.config == Config::Todo) {
            s.push_str(&format!("| {} | {} | {} | {} |\n", c.name, o.tokens, pct(o.recall), o.critical_missed()));
        }
    }

    s.push_str("\n## Fallos\n");
    let mut any = false;
    for c in &r.cases {
        let with_misses: Vec<&Outcome> = c.outcomes.iter().filter(|o| !o.misses.is_empty()).collect();
        if with_misses.is_empty() {
            continue;
        }
        any = true;
        s.push_str(&format!("\n### {}\n", c.name));
        for o in with_misses {
            let ms: Vec<String> = o
                .misses
                .iter()
                .map(|m| format!("L{}{} ({})", m.line, if m.critical { "!" } else { "" }, m.reason.describe()))
                .collect();
            s.push_str(&format!("- `{}`: {}\n", o.config.name(), ms.join(", ")));
        }
    }
    if !any {
        s.push_str("\nNinguno.\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "\
# Reglas
- Nunca hagas push directo a main.
- En `src/core/collider.rs` no uses unwrap.
- Todo `src/core/` es determinista, sin rand.
- Los botones de `src/ui/` usan el tema oscuro.
- Los secretos van en el gestor de secretos y jamás en el repositorio.
- Los botones de `src/ui/` usan el tema oscuro siempre.
";

    fn exists(p: &str) -> Option<String> {
        matches!(p, "src" | "src/core" | "src/core/collider.rs" | "src/ui" | "src/ui/button.rs").then(|| p.to_string())
    }

    #[test]
    fn parses_cases_and_reports_errors_with_line() {
        let f = parse_cases(
            "Notas libres.\ncore: L2\n\n## caso: uno\nprompt: arregla el #3\ngit: src\\core\\collider.rs\nlee: a.rs, b.rs # REVISAR\nrelevantes: L3, l4!\n\n## caso: dos\nrelevantes:\n",
        )
        .unwrap();
        assert_eq!(f.core, Some(vec![2]));
        assert_eq!(f.cases.len(), 2);
        let c = &f.cases[0];
        assert_eq!(c.prompt, "arregla el #3", "prompts keep their #");
        assert_eq!(c.git, vec!["src/core/collider.rs"]);
        assert_eq!(c.read, vec!["a.rs", "b.rs"]);
        assert_eq!(c.relevant, vec![(3, false), (4, true)]);
        assert!(f.cases[1].relevant.is_empty());

        assert_eq!(parse_cases("## caso: x\nrelevantes: L3, doce\n").unwrap_err(), "línea 2: `doce` no es una línea (usa L12 o L12!)");
        assert!(parse_cases("## caso: x\ncolor: rojo\n").unwrap_err().starts_with("línea 2: clave desconocida"));
        assert!(parse_cases("## caso: x\nsin dos puntos\n").unwrap_err().starts_with("línea 2:"));
        assert!(parse_cases("solo texto\n").is_err());
    }

    #[test]
    fn metrics_and_every_diagnosis() {
        // L2 core (push) · L3 collider · L4 core dir · L5 ui · L6 secrets (pathless, not core) · L7 merges into L5.
        let cases = parse_cases(
            "core: L2\n\
             ## caso: colisionador\nprompt: arréglalo\ngit: src/core/collider.rs\nrelevantes: L2!, L3, L4, L5, L6!, L7, L99\n",
        )
        .unwrap();
        let r = run("CLAUDE.md", MD, &cases, &exists, false);
        assert_eq!(r.merged, 1, "L7 repeats L5");
        let c = &r.cases[0];
        let get = |cfg: Config| c.outcomes.iter().find(|o| o.config == cfg).unwrap();

        let full = get(Config::Completo);
        assert_eq!(full.recall, 1.0);
        assert_eq!(full.tokens, text::approx_tokens(MD));

        let todo = get(Config::Todo);
        let reason = |o: &Outcome, l: usize| o.misses.iter().find(|m| m.line == l).map(|m| m.reason.clone());
        assert_eq!(reason(todo, 2), None, "core is injected");
        assert_eq!(reason(todo, 3), None, "touched file");
        assert_eq!(reason(todo, 4), None, "inherited from the directory");
        assert_eq!(reason(todo, 5), Some(MissReason::PathMismatch));
        assert_eq!(reason(todo, 6), Some(MissReason::PathlessNotCore));
        assert_eq!(reason(todo, 7), Some(MissReason::Merged(5)));
        assert_eq!(reason(todo, 99), Some(MissReason::NotImported));
        assert!((todo.recall - 3.0 / 7.0).abs() < 1e-6);
        assert_eq!(todo.critical_missed(), 1, "L6! lost");
        assert!((todo.precision - 3.0 / todo.injected as f32).abs() < 1e-6);

        let lex = get(Config::SoloLexico);
        assert_eq!(reason(lex, 3), Some(MissReason::NoLexicalMatch), "«arréglalo» names nothing");

        // No touched paths at all: a pathed rule missed by `todo` is a lexical miss, not a path one.
        let q = parse_cases("core: L2
## caso: pregunta
prompt: ¿qué aspecto tiene la interfaz?
relevantes: L5
").unwrap();
        let r2 = run("CLAUDE.md", MD, &q, &exists, false);
        let todo2 = r2.cases[0].outcomes.iter().find(|o| o.config == Config::Todo).unwrap();
        assert_eq!(reason(todo2, 5), Some(MissReason::NoLexicalMatch));

        let nested = get(Config::AnidadoIdeal);
        assert_eq!(reason(nested, 6), None, "per-folder files always carry the root rules");
        assert_eq!(reason(nested, 5), Some(MissReason::PathMismatch));
    }

    #[test]
    fn over_budget_and_low_chi_are_told_apart() {
        let mut md = String::from("# Reglas\n");
        for i in 0..60 {
            md.push_str(&format!("- En `src/core/collider.rs` alfa{i} beta{i} gamma{i} delta{i} epsilon{i} zeta{i} theta{i} kappa{i}.\n"));
        }
        let cases = parse_cases("## caso: muchas\ngit: src/core/collider.rs\nrelevantes: L2, L61\n").unwrap();
        let r = run("CLAUDE.md", &md, &cases, &exists, false);
        let todo = r.cases[0].outcomes.iter().find(|o| o.config == Config::Todo).unwrap();
        assert!(todo.misses.iter().any(|m| m.reason == MissReason::OverBudget), "{:?}", todo.misses);

        // A plan is invisible until it happens: χ = −1.
        let plan_md = "# Plan\n- [ ] Pendiente: migrar `src/core/collider.rs` a f64.\n";
        let cases = parse_cases("## caso: plan\ngit: src/core/collider.rs\nrelevantes: L2\n").unwrap();
        let r = run("CLAUDE.md", plan_md, &cases, &exists, false);
        let todo = r.cases[0].outcomes.iter().find(|o| o.config == Config::Todo).unwrap();
        assert_eq!(todo.misses[0].reason, MissReason::LowChi);
    }

    #[test]
    fn report_is_deterministic_and_marks_the_criterion() {
        let cases = parse_cases("core: L2\n## caso: a\nprompt: arréglalo\ngit: src/core/collider.rs\nrelevantes: L2!, L3\n## caso: b\nrelevantes:\n").unwrap();
        let a = render_markdown(&run("CLAUDE.md", MD, &cases, &exists, false), "prueba");
        let b = render_markdown(&run("CLAUDE.md", MD, &cases, &exists, false), "prueba");
        assert_eq!(a, b);
        assert!(a.contains("Sin etiquetar (no cuentan): b"));
        assert!(a.contains("Criterio (`todo`)"));
        assert!(a.contains("| todo |"));
    }
}
