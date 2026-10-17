//! `cthg` — project memory for coding agents.
//!
//! The memory of a repository lives in `<repo>/.cthg/memoria.cthg`. Commands print plain text
//! meant to be injected into the agent's context (hooks) or read by a person.

use cthg_core::learn::{self, Claim, Stance};
use cthg_core::model::*;
use cthg_core::retrieve::{self, Query};
use cthg_core::score::{self, certainty_tag, View};
use cthg_core::spatial::{self, Target};
use cthg_core::{binary, import, text};
use std::path::{Path, PathBuf};
use std::process::Command;

const DIR: &str = ".cthg";
const FILE: &str = "memoria.cthg";
/// Paths that change a lot and say nothing about the task.
const NOISE: &[&str] = &["Cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "poetry.lock"];
const NOISE_DIRS: &[&str] = &["target/", "node_modules/", "dist/", "build/", ".cthg/"];
const MAX_GIT_PATHS: usize = 12;

const HELP: &str = "cthg — memoria de proyecto (ruta · tiempo · confianza)

  cthg init
  cthg learn <texto> [--kind regla|decision|hecho|incidente|convencion|plan|comando]
             [--path P] [--symbol S] [--cause <por qué>] [--cause-id N]
             [--contradicts N] [--supersedes N] [--origin usuario|agente|repo|importado]
             [--core] [--plan] [--deny]
  cthg confirm <id>      cthg deny <id>
  cthg core [<id>...] [--budget N]
  cthg recall [--prompt <texto>] [--path P]... [--git] [--budget N] [--limit N]
  cthg list [--all]
  cthg mv <de> <a>       (la memoria sigue a un archivo o carpeta movidos)
  cthg import <archivo.md> [--yes]

Pendiente: hook (fase 4), verify (fase 5), gc y view (fase 7).";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("cthg: {e}");
        std::process::exit(1);
    }
}

/// Positional arguments plus `--name value` / `--flag` options (repeatable).
struct Args {
    pos: Vec<String>,
    opts: Vec<(String, Option<String>)>,
}

const FLAGS: &[&str] = &["core", "plan", "deny", "git", "all", "yes"];

impl Args {
    fn parse(raw: &[String]) -> Result<Self, String> {
        let mut pos = Vec::new();
        let mut opts = Vec::new();
        let mut it = raw.iter();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                if FLAGS.contains(&name) {
                    opts.push((name.to_string(), None));
                } else {
                    let v = it.next().ok_or(format!("falta el valor de --{name}"))?;
                    opts.push((name.to_string(), Some(v.clone())));
                }
            } else {
                pos.push(a.clone());
            }
        }
        Ok(Args { pos, opts })
    }
    fn flag(&self, k: &str) -> bool {
        self.opts.iter().any(|(n, _)| n == k)
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(n, _)| n == k).and_then(|(_, v)| v.as_deref())
    }
    fn all(&self, k: &str) -> Vec<&str> {
        self.opts.iter().filter(|(n, _)| n == k).filter_map(|(_, v)| v.as_deref()).collect()
    }
    fn num<T: std::str::FromStr>(&self, k: &str) -> Result<Option<T>, String> {
        self.get(k).map(|v| v.parse().map_err(|_| format!("--{k}: número inválido '{v}'"))).transpose()
    }
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The nearest folder with `.cthg/`, else the git root, else the current folder.
fn find_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Some(d) = cwd.ancestors().find(|d| d.join(DIR).is_dir()) {
        return d.to_path_buf();
    }
    git(&cwd, &["rev-parse", "--show-toplevel"])
        .map(|s| PathBuf::from(s.trim()))
        .unwrap_or(cwd)
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

struct Repo {
    root: PathBuf,
    file: PathBuf,
}

impl Repo {
    fn open() -> Self {
        let root = find_root();
        let file = root.join(DIR).join(FILE);
        Repo { root, file }
    }
    fn load(&self) -> Result<Memory, String> {
        if !self.file.exists() {
            return Err(format!("no hay memoria en {} (usa `cthg init`)", self.root.display()));
        }
        binary::load(&self.file).map_err(|e| e.to_string())
    }
    fn save(&self, m: &Memory) -> Result<(), String> {
        binary::save(m, &self.file).map_err(|e| e.to_string())
    }
    /// Current branch, or "" on trunk / outside git.
    fn branch(&self, m: &Memory) -> String {
        let cur = git(&self.root, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_default();
        if cur.is_empty() || cur == "HEAD" || Some(cur.as_str()) == m.meta("trunk") {
            String::new()
        } else {
            cur
        }
    }
    fn commit(&self) -> String {
        git(&self.root, &["rev-parse", "--short", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_default()
    }
    /// Applies any `git diff -M` / `git status` renames to `m` and returns ranked spatial targets
    /// with touched symbols from diff hunk headers.
    fn sync_git_and_targets(&self, m: &mut Memory) -> (Vec<Target>, usize) {
        let status = git(&self.root, &["status", "--porcelain=v1"]).unwrap_or_default();
        let diff = git(&self.root, &["diff", "-M", "-U0", "HEAD"]).unwrap_or_else(|| {
            let staged = git(&self.root, &["diff", "-M", "-U0", "--cached"]).unwrap_or_default();
            let unstaged = git(&self.root, &["diff", "-M", "-U0"]).unwrap_or_default();
            format!("{staged}\n{unstaged}")
        });
        let renames = spatial::parse_renames(&status, &diff);
        let moved = spatial::apply_renames(m, &renames, |p| self.root.join(p).exists());
        let targets = spatial::rank_git_targets(&status, &diff, NOISE, NOISE_DIRS, MAX_GIT_PATHS);
        (targets, moved)
    }
}

fn view(repo: &Repo, m: &Memory) -> View {
    View::on_branch(f64::INFINITY, &repo.branch(m))
}

fn parse_id(m: &Memory, s: Option<&String>) -> Result<u32, String> {
    let s = s.ok_or("falta el id")?;
    let id: u32 = s.trim_start_matches('#').parse().map_err(|_| format!("id inválido '{s}'"))?;
    m.node(id).map(|_| id).ok_or(format!("no existe el nodo #{id}"))
}

/// One injected line: `- regla (src/core/collider.rs::fn): texto ← causa`.
fn render(m: &Memory, scores: &[score::Score], n: &Node) -> String {
    let mut s = format!("- #{} {}", n.id, n.kind.name());
    if !n.path.is_empty() {
        s.push_str(&format!(" ({}", n.path));
        if !n.symbol.is_empty() {
            s.push_str(&format!("::{}", n.symbol));
        }
        s.push(')');
    }
    s.push_str(&format!(": {}", text::clip(&n.text, 240)));
    let cause = m
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Causes && e.to == n.id && scores[e.from as usize].visible)
        .max_by(|a, b| scores[a.from as usize].chi.partial_cmp(&scores[b.from as usize].chi).unwrap())
        .map(|e| &m.nodes[e.from as usize]);
    if let Some(c) = cause {
        s.push_str(&format!(" ← {}", text::clip(&c.label, 80)));
    }
    s
}

/// Lines grouped by χ band, most trusted band first.
fn print_bands(m: &Memory, scores: &[score::Score], ids: &[u32]) {
    let bands = ["verificado", "probable", "sin evidencia", "dudoso", "descartado"];
    for band in bands {
        let lines: Vec<String> = ids
            .iter()
            .filter(|&&id| certainty_tag(scores[id as usize].chi) == band)
            .map(|&id| render(m, scores, &m.nodes[id as usize]))
            .collect();
        if lines.is_empty() {
            continue;
        }
        let note = if band == "descartado" { " (ya se probó o se sustituyó: no repetir)" } else { "" };
        println!("[{band}]{note}");
        for l in lines {
            println!("{l}");
        }
    }
}

fn run(raw: &[String]) -> Result<(), String> {
    let Some((cmd, rest)) = raw.split_first() else {
        println!("{HELP}");
        return Ok(());
    };
    let a = Args::parse(rest)?;
    let repo = Repo::open();
    match cmd.as_str() {
        "init" => {
            if repo.file.exists() {
                println!("ya existe: {}", repo.file.display());
                return Ok(());
            }
            let mut m = Memory::default();
            let cur = git(&repo.root, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_string());
            let trunk = match cur.as_deref() {
                Some(b @ ("main" | "master" | "trunk" | "develop")) => b.to_string(),
                _ => "main".to_string(),
            };
            m.set_meta("trunk", &trunk);
            m.set_meta("created", &format!("{:.0}", now()));
            repo.save(&m)?;
            println!("memoria creada: {} (tronco: {trunk})", repo.file.display());
        }
        "learn" => {
            let mut m = repo.load()?;
            let body = a.pos.join(" ");
            if text::tokens(&body).len() < 2 {
                return Err("el texto necesita al menos dos palabras con contenido".into());
            }
            let mut c = Claim::rule(&body, a.get("path").unwrap_or(""));
            if let Some(k) = a.get("kind") {
                c.kind = NodeKind::parse(k).ok_or(format!("tipo desconocido '{k}'"))?;
            }
            if a.flag("plan") {
                c.kind = NodeKind::Plan;
            }
            if let Some(o) = a.get("origin") {
                c.origin = Origin::parse(o).ok_or(format!("origen desconocido '{o}'"))?;
            }
            c.symbol = a.get("symbol").unwrap_or("").to_string();
            c.branch = repo.branch(&m);
            c.commit = repo.commit();
            c.stance = if a.flag("deny") { Stance::Denies } else { Stance::Affirms };
            c.cause = a.get("cause").map(str::to_string);
            c.core = a.flag("core");
            for (k, slot) in [("cause-id", &mut c.cause_id), ("contradicts", &mut c.contradicts), ("supersedes", &mut c.supersedes)] {
                if let Some(v) = a.get(k) {
                    *slot = Some(parse_id(&m, Some(&v.to_string()))?);
                }
            }
            let events = learn::learn(&mut m, &c, now());
            repo.save(&m)?;
            for e in events {
                println!("{:<12} #{:<4} χ {:+.2} → {:+.2}  {}", e.action, e.node, e.chi_before, e.chi_after, e.label);
            }
        }
        "confirm" | "deny" => {
            let mut m = repo.load()?;
            let id = parse_id(&m, a.pos.first())?;
            let before = score::score_all(&m, &view(&repo, &m))[id as usize].chi;
            if !learn::feedback(&mut m, id, if cmd == "confirm" { 1 } else { -1 }) {
                return Err(format!("#{id} está anclado al repo: solo `verify` lo cambia"));
            }
            repo.save(&m)?;
            let after = score::score_all(&m, &view(&repo, &m))[id as usize].chi;
            println!("#{id} χ {before:+.2} → {after:+.2}");
        }
        "core" => {
            if !a.pos.is_empty() {
                let mut m = repo.load()?;
                let mut ids = Vec::new();
                for raw_id in &a.pos {
                    ids.push(parse_id(&m, Some(raw_id))?);
                }
                for &id in &ids {
                    m.nodes[id as usize].flags |= flags::CORE;
                }
                repo.save(&m)?;
                for id in ids {
                    println!("core         #{:<4} {}", id, m.nodes[id as usize].label);
                }
                return Ok(());
            }
            let m = repo.load()?;
            let scores = score::score_all(&m, &view(&repo, &m));
            let budget = a.num("budget")?.unwrap_or(500);
            let ids = retrieve::core(&m, &scores);
            let ids = retrieve::fit_budget(&m, &ids, budget, |n| render(&m, &scores, n));
            if !ids.is_empty() {
                println!("## Reglas del proyecto (siempre)");
                print_bands(&m, &scores, &ids);
            }
        }
        "recall" => {
            let mut m = repo.load()?;
            let mut targets: Vec<Target> = a.all("path").iter().map(|p| Target::new(p, 1.0)).collect();
            if a.flag("git") {
                let (git_ts, moved) = repo.sync_git_and_targets(&mut m);
                if moved > 0 {
                    repo.save(&m)?;
                }
                for t in git_ts {
                    if let Some(existing) = targets.iter_mut().find(|x| x.path == t.path) {
                        if existing.symbols.is_empty() {
                            existing.symbols = t.symbols;
                        }
                    } else {
                        targets.push(t);
                    }
                }
            }
            let scores = score::score_all(&m, &view(&repo, &m));
            let q = Query { text: a.get("prompt").unwrap_or("").to_string(), targets };
            let limit = a.num("limit")?.unwrap_or(40);
            let budget = a.num("budget")?.unwrap_or(800);
            let ids: Vec<u32> = retrieve::recall(&m, &scores, &q, limit, -1.0).iter().map(|r| r.id).collect();
            let ids = retrieve::fit_budget(&m, &ids, budget, |n| render(&m, &scores, n));
            if !ids.is_empty() {
                println!("## Memoria relevante");
                print_bands(&m, &scores, &ids);
            }
        }
        "list" => {
            let m = repo.load()?;
            let scores = if a.flag("all") { score::score_all(&m, &View::all()) } else { score::score_all(&m, &view(&repo, &m)) };
            for n in &m.nodes {
                let s = scores[n.id as usize];
                if !s.visible && !a.flag("all") {
                    continue;
                }
                let mut tags = Vec::new();
                for (f, t) in [(flags::CORE, "core"), (flags::ANCHORED, "anclado"), (flags::CONSOLIDATED, "consolidado"), (flags::POTENTIAL, "plan")] {
                    if n.has(f) {
                        tags.push(t);
                    }
                }
                let branch = if n.branch.is_empty() { String::new() } else { format!(" @{}", n.branch) };
                println!(
                    "#{:<4} χ {:+.2} {:<11} {:<10} {}{} [{}] {}",
                    n.id,
                    s.chi,
                    n.kind.name(),
                    n.origin.name(),
                    if n.path.is_empty() { "/" } else { &n.path },
                    branch,
                    tags.join(","),
                    n.label
                );
            }
        }
        "mv" => {
            let mut m = repo.load()?;
            let (Some(from), Some(to)) = (a.pos.first(), a.pos.get(1)) else {
                return Err("uso: cthg mv <de> <a>".into());
            };
            let moved = spatial::rename(&mut m, from, to);
            repo.save(&m)?;
            println!("{moved} recuerdos movidos de {from} a {to}");
        }
        "import" => {
            let mut m = repo.load()?;
            let file_arg = a.pos.first().ok_or("uso: cthg import <archivo.md> [--yes]")?;
            let arg_path = Path::new(file_arg);
            let full_path = if arg_path.exists() { arg_path.to_path_buf() } else { repo.root.join(arg_path) };
            let md = std::fs::read_to_string(&full_path)
                .map_err(|e| format!("no se pudo leer '{}': {e}", full_path.display()))?;
            let branch = repo.branch(&m);
            let commit = repo.commit();
            let apply_core = a.flag("yes");
            let report = import::import_markdown(&mut m, &md, &branch, &commit, now(), apply_core, |p| {
                repo.root.join(p).exists()
            });
            repo.save(&m)?;
            let scores = score::score_all(&m, &view(&repo, &m));
            for &id in &report.added {
                let n = &m.nodes[id as usize];
                let loc = if n.path.is_empty() {
                    "/".to_string()
                } else if n.symbol.is_empty() {
                    n.path.clone()
                } else {
                    format!("{}::{}", n.path, n.symbol)
                };
                println!(
                    "importado    #{:<4} χ {:+.2} {:<10} {:<24} {}",
                    n.id,
                    scores[id as usize].chi,
                    n.kind.name(),
                    loc,
                    n.label
                );
            }
            println!(
                "{} recuerdos importados ({} ya existían)",
                report.added.len(),
                report.existing.len()
            );
            if !report.marked_core.is_empty() {
                println!("{} marcados como CORE:", report.marked_core.len());
                for &id in &report.marked_core {
                    println!("  #{:<4} {}", id, m.nodes[id as usize].label);
                }
            }
            if !report.proposed_core.is_empty() {
                println!(
                    "\nPropuesta de CORE ({} reglas sin ruta con prohibiciones u obligaciones generales):",
                    report.proposed_core.len()
                );
                for &id in &report.proposed_core {
                    println!("  #{:<4} {}", id, m.nodes[id as usize].label);
                }
                println!(
                    "Para confirmarlas como CORE ejecuta:\n  cthg import {file_arg} --yes\no elige cuáles marcar con:\n  cthg core <id>..."
                );
            }
        }
        "hook" | "verify" | "gc" | "view" => {
            return Err(format!("`{cmd}` aún no está implementado (ver GUIA.md, sección Fases)"));
        }
        "help" | "--help" | "-h" => println!("{HELP}"),
        other => return Err(format!("comando desconocido '{other}'\n\n{HELP}")),
    }
    Ok(())
}
