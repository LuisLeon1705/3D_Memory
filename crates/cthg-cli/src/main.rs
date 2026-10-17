//! `cthg` — project memory for coding agents.
//!
//! The memory of a repository lives in `<repo>/.cthg/memoria.cthg`. Commands print plain text
//! meant to be injected into the agent's context (hooks) or read by a person.

use cthg_core::learn::{self, Claim, Stance};
use cthg_core::model::*;
use cthg_core::retrieve::{self, Query};
use cthg_core::score::{self, certainty_tag, View};
use cthg_core::spatial::{self, Target};
use cthg_core::verify::{self, RepoProbe, VerifyAction};
use cthg_core::{binary, eval, hook, import, text};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const DIR: &str = ".cthg";
const FILE: &str = "memoria.cthg";
const SES_DIR: &str = "sesion";
/// Paths that change a lot and say nothing about the task.
const NOISE: &[&str] = &["Cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "poetry.lock"];
const NOISE_DIRS: &[&str] = &["target/", "node_modules/", "dist/", "build/", ".cthg/"];
const MAX_GIT_PATHS: usize = 12;
/// A lock older than this was left by a crashed writer.
const STALE_LOCK: Duration = Duration::from_secs(30);
/// Hooks wait less than commands: if the memory is busy they answer read-only.
const HOOK_LOCK_WAIT: Duration = Duration::from_millis(1500);

const HELP: &str = "cthg — memoria de proyecto (ruta · tiempo · confianza)

  cthg init
  cthg learn <texto> [--kind regla|decision|hecho|incidente|convencion|plan|comando]
             [--path P] [--symbol S] [--cause <por qué>] [--cause-id N]
             [--contradicts N] [--supersedes N] [--origin usuario|agente|repo|importado]
             [--core] [--plan] [--deny]
  cthg confirm <id>      cthg deny <id>
  cthg core [<id>... [--unset]] [--budget N]
  cthg recall [--prompt <texto>] [--path P]... [--git] [--budget N] [--limit N]
  cthg list [--all]
  cthg mv <de> <a>       (la memoria sigue a un archivo o carpeta movidos)
  cthg import <archivo.md> [--yes]
  cthg hook <session-start|prompt|read|edit>
  cthg verify [--run-commands]
  cthg eval <carpeta> [--markdown] [--repo DIR] [--file CLAUDE.md]

Pendiente: gc y view (fase 7).";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("hook") {
        // Hooks never block or fail out loud: on any error, exit 0 with no output.
        let _ = run_hook(&args[1..]);
        return;
    }
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

const FLAGS: &[&str] = &["core", "plan", "deny", "git", "all", "yes", "unset", "run-commands", "markdown"];

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

/// The nearest folder with `.cthg/`, else the git root, else the starting folder.
fn find_root_from(start: Option<&Path>) -> PathBuf {
    let cwd = start
        .filter(|p| p.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
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

/// Held while the memory is being modified; dropping it releases `.cthg/lock`.
struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl Repo {
    fn open() -> Self {
        Self::open_at(None)
    }
    fn open_at(start: Option<&Path>) -> Self {
        let root = find_root_from(start);
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
    /// Exclusive `.cthg/lock` for one load-modify-save, waiting up to `wait`. Without it two
    /// writers (a hook and `cthg learn`) can each save their own copy and one change is lost.
    fn lock(&self, wait: Duration) -> Option<Lock> {
        let path = self.root.join(DIR).join("lock");
        let deadline = Instant::now() + wait;
        loop {
            match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Some(Lock(path)),
                Err(_) => {
                    let age = fs::metadata(&path).ok().and_then(|md| md.modified().ok()).and_then(|t| t.elapsed().ok());
                    if age.is_some_and(|a| a > STALE_LOCK) {
                        // Left behind by a writer that crashed.
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }
    /// Lock, then load: the memory to modify and the guard that must live until it is saved.
    fn load_locked(&self) -> Result<(Memory, Lock), String> {
        let lock = self
            .lock(Duration::from_secs(5))
            .ok_or("la memoria está en uso por otro proceso (.cthg/lock); inténtalo de nuevo")?;
        Ok((self.load()?, lock))
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
    /// Ranked spatial targets from `git status` / `git diff -M`, with the symbols that really
    /// enclose each hunk. On the trunk it also moves memories after renames; on another branch
    /// it does not, because the trunk still has the old paths (use `cthg mv` there).
    fn sync_git_and_targets(&self, m: &mut Memory) -> (Vec<Target>, usize) {
        let status = git(&self.root, &["status", "--porcelain=v1"]).unwrap_or_default();
        let diff = git(&self.root, &["diff", "-M", "-U0", "HEAD"]).unwrap_or_else(|| {
            let staged = git(&self.root, &["diff", "-M", "-U0", "--cached"]).unwrap_or_default();
            let unstaged = git(&self.root, &["diff", "-M", "-U0"]).unwrap_or_default();
            format!("{staged}\n{unstaged}")
        });
        let moved = if self.branch(m).is_empty() {
            let renames = spatial::parse_renames(&status, &diff);
            spatial::apply_renames(m, &renames, |p| self.root.join(p).exists())
        } else {
            0
        };
        let mut targets = spatial::rank_git_targets(&status, &diff, NOISE, NOISE_DIRS, MAX_GIT_PATHS);
        spatial::refine_symbols(&mut targets, &diff, |p| fs::read_to_string(self.root.join(p)).ok());
        (targets, moved)
    }
    /// Renames already committed, applied to memories whose path no longer exists (trunk only).
    fn follow_committed_renames(&self, m: &mut Memory) -> usize {
        if !self.branch(m).is_empty() {
            return 0;
        }
        let Some(log) = git(&self.root, &["log", "-M", "--diff-filter=R", "--name-status", "--format="]) else {
            return 0;
        };
        let renames: Vec<(String, String)> = spatial::parse_log_renames(&log)
            .into_iter()
            .filter(|(from, _)| !self.root.join(from).exists())
            .collect();
        spatial::apply_renames(m, &renames, |p| self.root.join(p).exists())
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
            let (mut m, _lock) = repo.load_locked()?;
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
            for e in &events {
                println!("{:<12} #{:<4} χ {:+.2} → {:+.2}  {}", e.action, e.node, e.chi_before, e.chi_after, e.label);
            }
            if c.origin == Origin::Repo {
                // "The repo proves it" is checked, not taken on faith.
                for e in events.iter().filter(|e| e.action == "nuevo") {
                    check_repo_claim(&repo, &mut m, e.node);
                }
            }
            repo.save(&m)?;
        }
        "confirm" | "deny" => {
            let (mut m, _lock) = repo.load_locked()?;
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
            let budget = a.num("budget")?.unwrap_or(hook::BUDGET_CORE);
            if !a.pos.is_empty() {
                let (mut m, _lock) = repo.load_locked()?;
                let mut ids = Vec::new();
                for raw_id in &a.pos {
                    ids.push(parse_id(&m, Some(raw_id))?);
                }
                let unset = a.flag("unset");
                for &id in &ids {
                    if unset {
                        m.nodes[id as usize].flags &= !flags::CORE;
                    } else {
                        m.nodes[id as usize].flags |= flags::CORE;
                    }
                }
                repo.save(&m)?;
                let action = if unset { "ya no core" } else { "core" };
                for id in ids {
                    println!("{action:<12} #{:<4} {}", id, m.nodes[id as usize].label);
                }
                warn_core_overflow(&m, &score::score_all(&m, &view(&repo, &m)), budget);
                return Ok(());
            }
            let m = repo.load()?;
            let scores = score::score_all(&m, &view(&repo, &m));
            let ids = retrieve::core(&m, &scores);
            let ids = retrieve::fit_budget(&m, &ids, budget, |n| render(&m, &scores, n));
            if !ids.is_empty() {
                println!("## Reglas del proyecto (siempre)");
                print_bands(&m, &scores, &ids);
            }
            warn_core_overflow(&m, &scores, budget);
        }
        "recall" => {
            // `--git` may move memories after renames, so it modifies and needs the lock.
            let (mut m, _lock) = if a.flag("git") {
                let (m, l) = repo.load_locked()?;
                (m, Some(l))
            } else {
                (repo.load()?, None)
            };
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
                for (f, t) in [
                    (flags::CORE, "core"),
                    (flags::ANCHORED, "anclado"),
                    (flags::CONSOLIDATED, "consolidado"),
                    (flags::POTENTIAL, "plan"),
                    (flags::STALE, "obsoleto"),
                ] {
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
            warn_core_overflow(&m, &scores, hook::BUDGET_CORE);
        }
        "mv" => {
            let (mut m, _lock) = repo.load_locked()?;
            let (Some(from), Some(to)) = (a.pos.first(), a.pos.get(1)) else {
                return Err("uso: cthg mv <de> <a>".into());
            };
            let moved = spatial::rename(&mut m, from, to);
            repo.save(&m)?;
            println!("{moved} recuerdos movidos de {from} a {to}");
        }
        "import" => {
            let (mut m, _lock) = repo.load_locked()?;
            let file_arg = a.pos.first().ok_or("uso: cthg import <archivo.md> [--yes]")?;
            let arg_path = Path::new(file_arg);
            let full_path = if arg_path.exists() { arg_path.to_path_buf() } else { repo.root.join(arg_path) };
            let md = std::fs::read_to_string(&full_path)
                .map_err(|e| format!("no se pudo leer '{}': {e}", full_path.display()))?;
            let branch = repo.branch(&m);
            let commit = repo.commit();
            let apply_core = a.flag("yes");
            // Each node remembers where it came from (`<archivo>:L<línea>`), so `eval` can refer to it.
            let source = spatial::normalize(file_arg);
            let report = import::import_markdown_named(&mut m, &source, &md, &branch, &commit, now(), apply_core, |p| {
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
            warn_core_overflow(&m, &scores, hook::BUDGET_CORE);
        }
        "verify" => {
            let (mut m, _lock) = repo.load_locked()?;
            let on_trunk = repo.branch(&m).is_empty();
            let moved = repo.follow_committed_renames(&mut m);
            if moved > 0 {
                println!("{moved} recuerdos siguieron a archivos renombrados en commits anteriores");
            }
            let probe = RepoProbe {
                path_exists: &|p| repo.root.join(p).exists(),
                read_file: &|p| fs::read_to_string(repo.root.join(p)).ok(),
                run_command: &|cmd| run_command(&repo.root, cmd),
            };
            let opts = verify::Options { apply: on_trunk, run_commands: a.flag("run-commands"), only: None };
            let v = view(&repo, &m);
            let report = verify::verify(&mut m, &v, &probe, &opts);
            repo.save(&m)?;
            if !on_trunk {
                println!("(rama {}: solo informe; anclas y obsolescencia se cambian en el tronco)", repo.branch(&m));
            }
            if report.events.is_empty() {
                println!("sin cambios: lo comprobable sigue igual que en la última verificación");
            }
            for e in &report.events {
                println!(
                    "{:<12} #{:<4} χ {:+.2} → {:+.2}  {} ({})",
                    e.action.name(),
                    e.node,
                    e.chi_before,
                    e.chi_after,
                    e.label,
                    e.reason
                );
                if e.action == VerifyAction::Unanchored {
                    println!("             ↳ se quitó el ancla (ANCHORED): única forma de que un hecho del repo pierda peso");
                }
            }
            if !report.skipped_commands.is_empty() {
                println!("\n{} comandos sin ejecutar (añade --run-commands para comprobarlos):", report.skipped_commands.len());
                for (id, cmd) in &report.skipped_commands {
                    println!("  #{id:<4} {cmd}");
                }
            }
        }
        "eval" => {
            let dir = PathBuf::from(a.pos.first().ok_or("uso: cthg eval <carpeta> [--markdown] [--repo DIR] [--file CLAUDE.md]")?);
            let name = match a.get("file") {
                Some(f) => f.to_string(),
                None => ["CLAUDE.md", "AGENTS.md"]
                    .into_iter()
                    .find(|f| dir.join(f).is_file())
                    .ok_or(format!("no hay CLAUDE.md ni AGENTS.md en {}", dir.display()))?
                    .to_string(),
            };
            let md = fs::read_to_string(dir.join(&name)).map_err(|e| format!("{name}: {e}"))?;
            let casos = fs::read_to_string(dir.join("casos.md")).map_err(|e| format!("casos.md: {e}"))?;
            let file = eval::parse_cases(&casos).map_err(|e| format!("casos.md, {e}"))?;
            // Which paths exist decides which rules get a place in the tree: from --repo, from
            // `rutas.txt` (one path per line) or from the current repository.
            let listed: Option<Vec<String>> = fs::read_to_string(dir.join("rutas.txt"))
                .ok()
                .map(|t| t.lines().map(spatial::normalize).filter(|p| !p.is_empty() && !p.starts_with('#')).collect());
            let root = a.get("repo").map(PathBuf::from).unwrap_or_else(|| repo.root.clone());
            let exists = |p: &str| match (&listed, a.get("repo")) {
                (Some(paths), None) => paths.iter().any(|x| spatial::is_ancestor_or_self(p, x)),
                _ => root.join(p).exists(),
            };
            let report = eval::run(&name, &md, &file, &exists);
            let out = eval::render_markdown(&report, &dir.display().to_string());
            print!("{out}");
            if a.flag("markdown") {
                let path = dir.join("resultado.md");
                fs::write(&path, &out).map_err(|e| format!("{}: {e}", path.display()))?;
                println!("
escrito: {}", path.display());
            }
        }
        "gc" | "view" => {
            return Err(format!("`{cmd}` aún no está implementado (ver GUIA.md, sección Fases)"));
        }
        "help" | "--help" | "-h" => println!("{HELP}"),
        other => return Err(format!("comando desconocido '{other}'\n\n{HELP}")),
    }
    Ok(())
}

/// Runs one memory command from the repo root. `verify` only passes commands without shell
/// syntax; this checks again because it is the last step before a process starts.
fn run_command(root: &Path, cmd: &str) -> Option<bool> {
    if !verify::is_safe_command(cmd) {
        return None;
    }
    let status = if cfg!(windows) {
        // `cmd /C` so `npm`, `pnpm`… (`.cmd` shims) resolve; nothing left in `cmd` can chain or redirect.
        Command::new("cmd").current_dir(root).args(["/C", cmd]).output().ok()?.status
    } else {
        let mut parts = cmd.split_whitespace();
        Command::new(parts.next()?).current_dir(root).args(parts).output().ok()?.status
    };
    Some(status.success())
}

/// After `learn --origin repo`: anchored only if the repo really shows it.
fn check_repo_claim(repo: &Repo, m: &mut Memory, id: u32) {
    let probe = RepoProbe {
        path_exists: &|p| repo.root.join(p).exists(),
        read_file: &|p| fs::read_to_string(repo.root.join(p)).ok(),
        run_command: &|_| None,
    };
    let opts = verify::Options { apply: true, run_commands: false, only: Some(vec![id]) };
    let v = view(repo, m);
    let report = verify::verify(m, &v, &probe, &opts);
    match report.events.iter().find(|e| e.node == id) {
        Some(e) if matches!(e.action, VerifyAction::Verified | VerifyAction::Anchored) => {
            println!("comprobado   #{id:<4} {}", e.reason);
        }
        Some(e) => println!("aviso: el repo no lo confirma ({}); #{id} queda sin ancla", e.reason),
        None => {
            // Nothing checkable (no path, symbol or dependency): it is the user's word, not the repo's.
            let n = &mut m.nodes[id as usize];
            n.flags &= !flags::ANCHORED;
            n.origin = Origin::User;
            (n.alpha, n.beta) = Origin::User.prior();
            println!("aviso: nada que comprobar en el repo; #{id} queda como afirmación del usuario");
        }
    }
}

/// Core rules that don't fit the session-start budget are never injected: say so, loudly.
fn warn_core_overflow(m: &Memory, scores: &[score::Score], budget: usize) {
    let all = retrieve::core(m, scores);
    let kept = retrieve::fit_budget(m, &all, budget, |n| render(m, scores, n));
    if kept.len() < all.len() {
        let dropped: Vec<String> = all.iter().filter(|id| !kept.contains(id)).map(|id| format!("#{id}")).collect();
        eprintln!(
            "aviso: {} de {} reglas CORE no caben en {budget} tokens y no se inyectarán: {}\n       quita alguna con `cthg core --unset <id>` o dale una ruta",
            dropped.len(),
            all.len(),
            dropped.join(" ")
        );
    }
}

fn run_hook(rest: &[String]) -> Result<(), String> {
    let event = rest.first().map(String::as_str).unwrap_or("");
    let mut raw_json = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw_json);
    let input = hook::HookInput::parse(&raw_json);
    let start = (!input.cwd.is_empty()).then(|| Path::new(&input.cwd));
    let repo = Repo::open_at(start);
    // Busy memory: still answer, but read-only (the context matters more than the activation).
    let lock = repo.lock(HOOK_LOCK_WAIT);
    let mut m = repo.load()?;
    let ses_dir = repo.root.join(DIR).join(SES_DIR);
    let save = |m: &Memory| {
        if lock.is_some() {
            let _ = repo.save(m);
        }
    };

    match event {
        "session-start" => {
            let mut session = hook::SessionState::new(&input.session_id);
            let v = view(&repo, &m);
            let out = hook::handle_session_start(&mut m, &v, &mut session);
            let _ = session.save(&ses_dir);
            save(&m);
            if !out.is_empty() {
                println!("{out}");
            }
        }
        "prompt" => {
            let mut session = hook::SessionState::load(&ses_dir, &input.session_id);
            let (git_ts, moved) = repo.sync_git_and_targets(&mut m);
            let v = view(&repo, &m);
            let out = hook::handle_prompt(&mut m, &v, &mut session, &input.prompt, git_ts);
            if !out.is_empty() || moved > 0 {
                save(&m);
            }
            if !out.is_empty() {
                let _ = session.save(&ses_dir);
                println!("{out}");
            }
        }
        "read" => {
            let rel = hook::relativize_path(&repo.root, &input.file_path);
            let mut session = hook::SessionState::load(&ses_dir, &input.session_id);
            let v = view(&repo, &m);
            let out = hook::handle_read(&mut m, &v, &mut session, &rel);
            if !out.is_empty() {
                let _ = session.save(&ses_dir);
                save(&m);
                println!("{out}");
            }
        }
        "edit" => {
            let rel = hook::relativize_path(&repo.root, &input.file_path);
            if hook::handle_edit(&mut m, &rel) > 0 {
                save(&m);
            }
        }
        _ => {}
    }
    Ok(())
}
