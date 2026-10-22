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
use cthg_core::{binary, correction, eval, hook, import, index, text};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const MAP_HINT: &str = "Mapa del proyecto: antes de escribir un archivo, `cthg map <ruta>` trae sus reglas y vecinos; `cthg where/callers <símbolo>` en vez de grep.";
const DIR: &str = ".cthg";
const FILE: &str = "memoria.cthg";
const SES_DIR: &str = "sesion";
/// Paths that change a lot and say nothing about the task.
const NOISE: &[&str] = &["Cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "poetry.lock"];
const NOISE_DIRS: &[&str] = &[
    "target/", "node_modules/", "dist/", "build/", ".cthg/",
    "__pycache__/", ".venv/", "venv/", "env/", ".next/",
    "coverage/", ".mypy_cache/", ".pytest_cache/",
];
const MAX_GIT_PATHS: usize = 12;
/// Corrections already learned in this repo, one fingerprint per line: a second `Stop` over the
/// same transcript, or a resumed session, does not learn them again.
const LEDGER: &str = "correcciones.txt";
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
  cthg supersede <nuevo> <viejo>
  cthg core [<id>... [--unset]] [--budget N]
  cthg recall [--prompt <texto>] [--path P]... [--git] [--budget N] [--limit N] [--sin-vecinos]
  cthg index [--rebuild]
  cthg map [ruta] [--limit N]
  cthg where <símbolo>
  cthg callers <símbolo> [--tests] [--posibles]
  cthg list [--all]
  cthg mv <de> <a>       (la memoria sigue a un archivo o carpeta movidos)
  cthg import <archivo.md> [--yes]
  cthg hook <session-start|prompt|read|edit|stop>    (Claude Code)
  cthg hook antigravity                              (Google Antigravity, PreInvocation)
  cthg hook antigravity-stop                         (Google Antigravity, Stop)
  cthg verify [--run-commands]
  cthg <init|learn|confirm|deny|supersede|core|list|import> --global   (memoria de todos los proyectos)
  cthg eval <carpeta> [--markdown] [--repo DIR] [--file CLAUDE.md] [--core-auto] [--w-vecino X] [--w-simbolo Y]
  cthg corrections [<transcript.jsonl|carpeta>...] [--desde AAAA-MM-DD] [--detalle]
      (correcciones del usuario por conversación; sin rutas, los transcripts de Antigravity)

Pendiente: gc y view (fase 7).";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("hook") {
        // Hooks never block or fail out loud: on any error, exit 0 with no output.
        std::panic::set_hook(Box::new(|_| {}));
        let _ = std::panic::catch_unwind(|| {
            let _ = run_hook(&args[1..]);
        });
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

const FLAGS: &[&str] = &["core", "plan", "deny", "git", "all", "yes", "unset", "run-commands", "markdown", "core-auto", "global", "detalle", "rebuild", "tests", "sin-vecinos"];

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

/// The nearest folder with `.cthg/memoria.cthg`, else the git root, else the starting folder.
fn find_root_from(start: Option<&Path>) -> PathBuf {
    let cwd = start
        .filter(|p| p.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    // A project is a folder with `.cthg/memoria.cthg`, not just `.cthg/`: the global memory
    // lives in `~/.cthg/`, an ancestor of every project, and must never be taken for one.
    if let Some(d) = cwd.ancestors().find(|d| d.join(DIR).join(FILE).is_file()) {
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
    /// The user's memory shared by every project (`--global`): no git, no branches, no paths.
    global: bool,
}

/// The global memory: `CTHG_GLOBAL`, else `~/.cthg/global.cthg`.
fn global_file() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CTHG_GLOBAL") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    Some(Path::new(&home).join(DIR).join("global.cthg"))
}

/// The global memory, if it exists and reads. Hooks inject its `CORE` before the project's.
fn load_global() -> Option<Memory> {
    global_file().filter(|f| f.is_file()).and_then(|f| binary::load(&f).ok())
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
        Repo { root, file, global: false }
    }
    fn global() -> Result<Self, String> {
        let file = global_file().ok_or("no sé dónde va la memoria global: define CTHG_GLOBAL o USERPROFILE/HOME")?;
        let root = file.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(Repo { root, file, global: true })
    }
    fn load(&self) -> Result<Memory, String> {
        if !self.file.exists() {
            let init = if self.global { "cthg init --global" } else { "cthg init" };
            return Err(format!("no hay memoria en {} (usa `{init}`)", self.file.display()));
        }
        binary::load(&self.file).map_err(|e| e.to_string())
    }
    fn save(&self, m: &Memory) -> Result<(), String> {
        binary::save(m, &self.file).map_err(|e| e.to_string())
    }
    /// Exclusive `.cthg/lock` for one load-modify-save, waiting up to `wait`. Without it two
    /// writers (a hook and `cthg learn`) can each save their own copy and one change is lost.
    fn lock(&self, wait: Duration) -> Option<Lock> {
        // Next to the memory file: `.cthg/lock` for a project, `~/.cthg/lock` for the global one.
        let path = self.file.with_file_name("lock");
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
        if self.global {
            return String::new();
        }
        let cur = git(&self.root, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_default();
        if cur.is_empty() || cur == "HEAD" || Some(cur.as_str()) == m.meta("trunk") {
            String::new()
        } else {
            cur
        }
    }
    fn commit(&self) -> String {
        if self.global {
            return String::new();
        }
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

/// One injected line: `- regla (src/core/collider.rs::fn): texto ← causa`, exactly as the hooks print it.
fn render(m: &Memory, scores: &[score::Score], n: &Node) -> String {
    hook::render_node(m, scores, n)
}

/// Lines grouped by χ band, most trusted band first.
fn print_bands(m: &Memory, scores: &[score::Score], ids: &[u32]) {
    let empty_map = std::collections::HashMap::new();
    print_bands_with_neighbors(m, scores, ids, &empty_map);
}

fn print_bands_with_neighbors(
    m: &Memory,
    scores: &[score::Score],
    ids: &[u32],
    neighbor_origins: &std::collections::HashMap<u32, String>,
) {
    let bands = ["verificado", "probable", "sin evidencia", "dudoso", "descartado"];
    for band in bands {
        let lines: Vec<String> = ids
            .iter()
            .filter(|&&id| certainty_tag(scores[id as usize].chi) == band)
            .map(|&id| {
                hook::render_node_with_neighbor(
                    m,
                    scores,
                    &m.nodes[id as usize],
                    neighbor_origins.get(&id).map(String::as_str),
                )
            })
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
    let repo = if a.flag("global") { Repo::global()? } else { Repo::open() };
    if repo.global {
        if matches!(cmd.as_str(), "recall" | "verify" | "mv" | "eval" | "hook" | "corrections" | "index" | "map" | "where" | "callers") {
            return Err(format!("`{cmd}` no aplica a la memoria global: no tiene rutas ni repo"));
        }
        if a.get("path").is_some() || a.get("symbol").is_some() {
            return Err("la memoria global no tiene rutas: sus reglas valen en todos los proyectos".into());
        }
    }
    match cmd.as_str() {
        "init" => {
            if repo.file.exists() {
                println!("ya existe: {}", repo.file.display());
                return Ok(());
            }
            let mut m = Memory::default();
            if repo.global {
                m.set_meta("scope", "global");
                m.set_meta("created", &format!("{:.0}", now()));
                repo.save(&m)?;
                println!("memoria global creada: {}", repo.file.display());
                return Ok(());
            }
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
            let proposed = m.nodes[id as usize].has(flags::PROPOSED);
            if !learn::feedback(&mut m, id, if cmd == "confirm" { 1 } else { -1 }) {
                return Err(format!("#{id} está anclado al repo: solo `verify` lo cambia"));
            }
            repo.save(&m)?;
            let after = score::score_all(&m, &view(&repo, &m))[id as usize].chi;
            println!("#{id} χ {before:+.2} → {after:+.2}");
            if proposed && cmd == "confirm" {
                println!("   era una corrección capturada: desde ahora se inyecta como las demás");
            }
        }
        "supersede" => {
            let (mut m, _lock) = repo.load_locked()?;
            let (Some(new_str), Some(old_str)) = (a.pos.first(), a.pos.get(1)) else {
                return Err("uso: cthg supersede <nuevo> <viejo>".into());
            };
            let new_id = parse_id(&m, Some(new_str))?;
            let old_id = parse_id(&m, Some(old_str))?;
            let v = view(&repo, &m);
            if !v.sees(&m.nodes[new_id as usize]) {
                return Err(format!("#{new_id} no es visible en esta vista"));
            }
            if !v.sees(&m.nodes[old_id as usize]) {
                return Err(format!("#{old_id} no es visible en esta vista"));
            }
            let before = score::score_all(&m, &v)[old_id as usize].chi;
            learn::supersede(&mut m, new_id, old_id)?;
            repo.save(&m)?;
            let after = score::score_all(&m, &v)[old_id as usize].chi;
            println!("#{old_id} χ {before:+.2} → {after:+.2} sustituido por #{new_id}");
        }
        "core" => {
            let budget_arg: Option<usize> = a.num("budget")?;
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
                let budget = budget_arg.unwrap_or(core_budget(&repo, &m));
                warn_core_overflow(&m, &score::score_all(&m, &view(&repo, &m)), budget);
                return Ok(());
            }
            let m = repo.load()?;
            let scores = score::score_all(&m, &view(&repo, &m));
            let budget = budget_arg.unwrap_or(core_budget(&repo, &m));
            // Exactly what session start injects: the global layer first (its own budget), then this memory.
            let (global_out, _, global_toks) = match (repo.global, load_global()) {
                (false, Some(g)) => hook::global_core(&g, hook::GLOBAL_CORE_MAX),
                _ => Default::default(),
            };
            if !global_out.is_empty() {
                println!("{global_out}\n");
            }
            let ids = project_core(&m, &scores, &global_toks);
            let shown = retrieve::fit_budget(&m, &ids, budget, |n| render(&m, &scores, n));
            if !shown.is_empty() {
                println!("{}", if repo.global { "## Reglas globales (siempre, en todos los proyectos)" } else { "## Reglas del proyecto (siempre)" });
                print_bands(&m, &scores, &shown);
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
            let index = if a.flag("sin-vecinos") {
                None
            } else {
                load_index_cached(&repo.root)
            };
            if let Some(ref idx) = index {
                let mut all_targets = spatial::expand_neighbors(&targets, Some(idx), spatial::W_NEIGHBOR, spatial::MAX_NEIGHBORS);
                if let Some(prompt) = a.get("prompt") {
                    let id_targets = spatial::identifier_targets(prompt, Some(idx), spatial::W_SYMBOL, spatial::MIN_IDENT_LEN, spatial::MAX_DEFS);
                    spatial::merge_targets(&mut all_targets, id_targets);
                }
                targets = all_targets;
            }
            let scores = score::score_all(&m, &view(&repo, &m));
            let q = Query { text: a.get("prompt").unwrap_or("").to_string(), targets };
            let limit = a.num("limit")?.unwrap_or(40);
            let budget = a.num("budget")?.unwrap_or(hook::Budgets::of(&m).for_prompt(a.get("prompt").unwrap_or("")));
            // Like the message hook: unconfirmed proposals stay out.
            let ids: Vec<u32> = retrieve::recall(&m, &scores, &q, limit, -1.0)
                .iter()
                .map(|r| r.id)
                .filter(|&id| hook::may_inject(&m.nodes[id as usize], &scores[id as usize], None))
                .collect();
            let neighbor_origins = spatial::node_neighbor_origins(&m, &q.targets, &ids);
            let ids = retrieve::fit_budget(&m, &ids, budget, |n| {
                hook::render_node_with_neighbor(&m, &scores, n, neighbor_origins.get(&n.id).map(String::as_str))
            });
            if !ids.is_empty() {
                println!("## Memoria relevante");
                print_bands_with_neighbors(&m, &scores, &ids, &neighbor_origins);
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
                    (flags::PROPOSED, "propuesta"),
                ] {
                    if n.has(f) {
                        tags.push(t);
                    }
                }
                if s.superseded {
                    tags.push("sustituido");
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
            warn_core_overflow(&m, &scores, core_budget(&repo, &m));
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
            let files = repo_files(&repo.root);
            let resolve = |p: &str| resolve_in(&files, &repo.root, p);
            // The CORE proposal fits in the budget the memory will have once imported.
            let report = import::import_markdown_in(&mut m, &source, &md, &branch, &commit, now(), apply_core, None, resolve);
            repo.save(&m)?;
            let scores = score::score_all(&m, &view(&repo, &m));
            let line = |action: &str, id: u32| {
                let n = &m.nodes[id as usize];
                let loc = if n.path.is_empty() {
                    "/".to_string()
                } else if n.symbol.is_empty() {
                    n.path.clone()
                } else {
                    format!("{}::{}", n.path, n.symbol)
                };
                println!(
                    "{action:<12} #{:<4} χ {:+.2} {:<10} {:<24} {}",
                    n.id,
                    scores[id as usize].chi,
                    n.kind.name(),
                    loc,
                    n.label
                );
            };
            for &id in &report.added {
                line("importado", id);
            }
            for &(old, new) in &report.updated {
                line("actualizado", new);
                let note = if m.nodes[old as usize].anchored() { " (anclado: sigue en pie hasta que verify le quite el ancla)" } else { "" };
                println!("{:<12} └ sustituye a #{} (χ {:+.2}){note}", "", old, scores[old as usize].chi);
            }
            for &id in &report.stale {
                line("obsoleto", id);
            }
            for &id in &report.restored {
                line("restaurado", id);
            }
            for &id in &report.missing {
                line("ya no está", id);
            }
            if !report.missing.is_empty() {
                if branch.is_empty() {
                    println!("  (anclados: solo verify los mueve)");
                } else {
                    println!("  (en la rama {branch} no se marcan: se marcan al importar en el tronco)");
                }
            }
            println!(
                "{} nuevos, {} actualizados, {} obsoletos, {} restaurados ({} ya existían)",
                report.added.len(),
                report.updated.len(),
                report.stale.len(),
                report.restored.len(),
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
            warn_core_overflow(&m, &scores, core_budget(&repo, &m));
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
            let dir = PathBuf::from(a.pos.first().ok_or("uso: cthg eval <carpeta> [--markdown] [--repo DIR] [--file CLAUDE.md] [--core-auto]")?);
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
            let files = match (listed, a.get("repo")) {
                (Some(paths), None) => Some(import::RepoFiles::new(paths)),
                _ => repo_files(&root),
            };
            let resolve = |p: &str| resolve_in(&files, &root, p);
            let index = if a.get("repo").is_some() {
                let list_files_fn = || {
                    if let Some(ref rf) = repo_files(&root) {
                        rf.files().to_vec()
                    } else {
                        let mut out = Vec::new();
                        walk_dir_rs(&root, "", &mut out);
                        out
                    }
                };
                let file_meta_fn = |p: &str| {
                    let meta = fs::metadata(root.join(p)).ok()?;
                    let size = meta.len();
                    let mtime = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    Some((size, mtime))
                };
                let read_file_bytes_fn = |p: &str| fs::read(root.join(p)).ok();
                let read_index_fn = || load_index_cached(&root).map(|idx| index::format_index(&idx));
                let write_index_fn = |_content: &str| Ok(());

                let probe = index::IndexProbe {
                    list_files: &list_files_fn,
                    file_meta: &file_meta_fn,
                    read_file_bytes: &read_file_bytes_fn,
                    read_index: &read_index_fn,
                    write_index: &write_index_fn,
                };
                index::update_index(&probe, false).ok().map(|(idx, _)| idx)
            } else {
                None
            };
            let w_vecino = a.num::<f32>("w-vecino")?.unwrap_or(cthg_core::spatial::W_NEIGHBOR);
            let w_simbolo = a.num::<f32>("w-simbolo")?.unwrap_or(cthg_core::spatial::W_SYMBOL);
            let weights = eval::EvalWeights {
                w_neighbor: w_vecino,
                w_symbol: w_simbolo,
            };
            let report = eval::run_with_index_and_weights(&name, &md, &file, &resolve, a.flag("core-auto"), index.as_ref(), weights);
            let out = eval::render_markdown(&report, &dir.display().to_string());
            print!("{out}");
            if a.flag("markdown") {
                let path = dir.join("resultado.md");
                fs::write(&path, &out).map_err(|e| format!("{}: {e}", path.display()))?;
                println!("
escrito: {}", path.display());
            }
        }
        "corrections" => {
            let files: Vec<PathBuf> = if a.pos.is_empty() {
                antigravity_transcripts()
            } else {
                a.pos.iter().flat_map(|p| jsonl_under(Path::new(p))).collect()
            };
            if files.is_empty() {
                return Err("no encontré transcripts (~/.gemini/antigravity*/brain/*/.system_generated/logs/transcript.jsonl); pasa archivos o carpetas".into());
            }
            let stats: Vec<correction::ConvStats> = files
                .iter()
                .filter_map(|f| {
                    let raw = fs::read(f).ok()?;
                    Some(correction::stats(&conversation_name(f), &correction::parse(&String::from_utf8_lossy(&raw))))
                })
                .collect();
            print!("{}", correction::report(&stats, a.get("desde"), a.flag("detalle")));
        }
        "index" => {
            let start = Instant::now();
            let rebuild = a.flag("rebuild");
            let (_idx, stats) = run_index_update(&repo.root, rebuild)?;
            let elapsed_ms = start.elapsed().as_millis();
            println!(
                "índice: {} archivos ({} reescaneados, {} sin cambios, {} quitados), {} símbolos, {} importaciones resueltas de {}, en {} ms",
                stats.total_files,
                stats.rescanned,
                stats.unchanged,
                stats.removed,
                stats.total_symbols,
                stats.resolved_imports,
                stats.total_imports,
                elapsed_ms
            );
            if !stats.failed_files.is_empty() {
                println!(
                    "{} archivos no se pudieron escanear: {}",
                    stats.failed_files.len(),
                    stats.failed_files.join(", ")
                );
            }
        }
        "where" => {
            let sym = a.pos.first().ok_or_else(|| "falta el símbolo: `cthg where <símbolo>`".to_string())?;
            let (idx, _) = run_index_update(&repo.root, false)?;
            let results = index::where_symbol(&idx, sym);
            if results.is_empty() {
                println!("no se encontraron definiciones de '{sym}'");
            } else {
                for r in results {
                    let parent_str = r.parent.as_ref().map(|p| format!(" [{p}]")).unwrap_or_default();
                    let sig_str = if r.signature.is_empty() {
                        String::new()
                    } else if r.signature.starts_with('(') || r.signature.starts_with('<') || r.signature.starts_with(':') {
                        r.signature.clone()
                    } else {
                        format!(" {}", r.signature)
                    };
                    println!("{}:{} {} {}{}{}", r.path, r.line, r.kind, r.name, sig_str, parent_str);
                }
            }
        }
        "callers" => {
            let sym = a.pos.first().ok_or_else(|| "falta el símbolo: `cthg callers <símbolo>`".to_string())?;
            let (idx, _) = run_index_update(&repo.root, false)?;
            let res = index::callers(&idx, sym);
            if res.is_ambiguous {
                println!(
                    "aviso: '{sym}' está definido en {} archivos (ambiguo; priorizando llamadas desde importadores)",
                    res.def_files_count
                );
            }
            let show_tests = a.flag("tests");
            let show_posibles = a.flag("posibles");
            let normal_callers: Vec<_> = res.callers.iter().filter(|c| !c.is_test).collect();
            let test_callers: Vec<_> = res.callers.iter().filter(|c| c.is_test).collect();
            let normal_posibles: Vec<_> = res.posibles.iter().filter(|c| !c.is_test).collect();
            let test_posibles: Vec<_> = res.posibles.iter().filter(|c| c.is_test).collect();

            if normal_callers.is_empty()
                && test_callers.is_empty()
                && normal_posibles.is_empty()
                && test_posibles.is_empty()
            {
                println!("no se encontraron llamadas a '{sym}'");
                return Ok(());
            }

            for c in normal_callers {
                if c.from == "(nivel de módulo)" {
                    println!("{}:{} (nivel de módulo)", c.path, c.line);
                } else {
                    println!("{}:{} (dentro de {})", c.path, c.line, c.from);
                }
            }

            if show_tests {
                if !test_callers.is_empty() {
                    println!("[en tests]");
                    for c in test_callers {
                        if c.from == "(nivel de módulo)" {
                            println!("{}:{} (nivel de módulo)", c.path, c.line);
                        } else {
                            println!("{}:{} (dentro de {})", c.path, c.line, c.from);
                        }
                    }
                }
            } else if !test_callers.is_empty() {
                println!("({} llamadas en tests, usa --tests para verlas)", test_callers.len());
            }

            if show_posibles {
                let list = if show_tests {
                    let mut all = normal_posibles;
                    all.extend(test_posibles);
                    all
                } else {
                    normal_posibles
                };
                if !list.is_empty() {
                    println!("[posibles (por nombre)]");
                    for c in list {
                        if c.from == "(nivel de módulo)" {
                            println!("{}:{} (nivel de módulo)", c.path, c.line);
                        } else {
                            println!("{}:{} (dentro de {})", c.path, c.line, c.from);
                        }
                    }
                }
            } else {
                let total = normal_posibles.len() + if show_tests { test_posibles.len() } else { 0 };
                let count = if show_tests { total } else { normal_posibles.len() };
                if count > 0 {
                    println!("({} posibles por nombre, usa --posibles para verlas)", count);
                }
            }
        }
        "map" => {
            let (idx, _) = run_index_update(&repo.root, false)?;
            let raw_target = a.pos.first().map(|s| spatial::normalize(s));
            let target = raw_target.as_deref().unwrap_or("");

            let is_indexed = idx.get_file(target).is_some();
            let exists_on_disk = !target.is_empty() && repo.root.join(target).exists();
            let is_dir_on_disk = !target.is_empty() && repo.root.join(target).is_dir();
            let is_tree_query = target.is_empty()
                || is_dir_on_disk
                || (!is_indexed && !exists_on_disk && idx.files.iter().any(|f| f.path.starts_with(&format!("{target}/"))));

            if !is_tree_query {
                let m_opt = if repo.file.exists() { repo.load().ok() } else { None };
                let view_m;
                let scores = if let Some(ref m) = m_opt {
                    view_m = view(&repo, m);
                    score::score_all(m, &view_m)
                } else {
                    Vec::new()
                };

                let res = index::file_map(&idx, m_opt.as_ref(), &scores, target, exists_on_disk);
                print_file_map_result(&res, a.flag("tests"), m_opt.as_ref(), Some(&scores));
            } else {
                let limit = a.num("limit")?.unwrap_or(50);
                let tree = index::compact_tree(&idx, target, limit);
                if tree.is_empty() {
                    if target.is_empty() {
                        println!("el repositorio no tiene archivos indexados");
                    } else {
                        println!("no hay archivos indexados en '{target}'");
                    }
                    return Ok(());
                }
                for f in &tree {
                    println!("{} ({} símbolos, {} públicos):", f.path, f.total_symbols, f.public_symbols.len());
                    for s in &f.public_symbols {
                        let parent_str = s.parent.as_ref().map(|p| format!(" [{p}]")).unwrap_or_default();
                        let sig_100 = truncate_sig_map(&s.signature);
                        println!("  pub {} {}{sig_100} (L{}){}", s.kind, s.name, s.line, parent_str);
                    }
                }
                if tree.len() >= limit {
                    println!("... (recortado a {limit} archivos, usa --limit N para ver más)");
                }
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

/// The repo's files (tracked, and untracked but not ignored), so `import` can tell which paths
/// exist and find a bare file name. `None` outside git.
fn repo_files(root: &Path) -> Option<import::RepoFiles> {
    let out = git(root, &["ls-files", "--cached", "--others", "--exclude-standard"])?;
    Some(import::RepoFiles::new(out.lines().map(str::to_string)))
}

/// With a file list, a bare name can resolve to its one file; without it, only exact paths count.
fn resolve_in(files: &Option<import::RepoFiles>, root: &Path, p: &str) -> Option<String> {
    match files {
        Some(f) => f.resolve(p),
        None => root.join(p).exists().then(|| p.to_string()),
    }
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

/// The `CORE` budget of a memory: the global one has a fixed one, a project's grows with it.
fn core_budget(repo: &Repo, m: &Memory) -> usize {
    if repo.global {
        hook::GLOBAL_CORE_MAX
    } else {
        hook::Budgets::of(m).core
    }
}

/// This memory's `CORE`, minus the rules that repeat a global one (they are injected once).
fn project_core(m: &Memory, scores: &[score::Score], global_toks: &[Vec<String>]) -> Vec<u32> {
    retrieve::core(m, scores)
        .into_iter()
        .filter(|&id| hook::may_inject(&m.nodes[id as usize], &scores[id as usize], None))
        .filter(|&id| {
            let toks = text::tokens(&m.nodes[id as usize].text);
            !global_toks.iter().any(|g| text::similarity(&toks, g) >= hook::DUPLICATE_OF_GLOBAL)
        })
        .collect()
}

/// Core rules that don't fit the session-start budget are never injected: say so, loudly.
fn warn_core_overflow(m: &Memory, scores: &[score::Score], budget: usize) {
    let global_toks = load_global().map(|g| hook::global_core(&g, hook::GLOBAL_CORE_MAX).2).unwrap_or_default();
    let all = project_core(m, scores, &global_toks);
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

/// Antigravity's `PreInvocation` hook (see `cthg_core::antigravity`): reads the transcript lines
/// added since the last call and turns them into one ephemeral message.
fn run_antigravity(raw: &str) -> Result<String, String> {
    use cthg_core::antigravity::{self as agy, Step};
    let p = agy::parse_payload(raw);
    let start = (!p.workspace.is_empty()).then(|| Path::new(&p.workspace));
    let repo = Repo::open_at(start);
    let lock = repo.lock(HOOK_LOCK_WAIT);
    let mut m = repo.load()?;
    let ses_dir = repo.root.join(DIR).join(SES_DIR);
    let sid = if p.conversation.is_empty() { "antigravity" } else { p.conversation.as_str() };
    let mut session = hook::SessionState::load(&ses_dir, sid);
    session.native = ANTIGRAVITY_NATIVE.iter().map(|s| s.to_string()).collect();

    // Only what was appended since the last call.
    let mut steps = Vec::new();
    if let Ok(bytes) = fs::read(&p.transcript) {
        if (bytes.len() as u64) < session.offset {
            session.offset = 0; // a new or rewritten transcript
        }
        let fresh = String::from_utf8_lossy(&bytes[session.offset as usize..]).into_owned();
        let (found, consumed) = agy::scan(&fresh);
        session.offset += consumed as u64;
        steps = found;
    }

    let mut index_opt = None;
    if !session.started {
        learn::begin_session(&mut m);
        session.started = true;
        index_opt = update_index_with_timeout(&repo.root, Duration::from_millis(500));
    }
    let idx = index_opt.or_else(|| load_index_cached(&repo.root));
    let v = view(&repo, &m);
    let global = load_global();
    let mut parts: Vec<String> = Vec::new();
    // Injected messages stay in the conversation (the transcript keeps them as
    // `EPHEMERAL_MESSAGE` steps and the model still sees them next turn): the CORE goes once,
    // and nothing already injected in this conversation is injected again.
    if session.turn == -1 {
        let (mut core, global_ids) = hook::core_context_with_global(&mut m, &v, &mut session, global.as_ref());
        if !global_ids.is_empty() {
            hook::record_global_recalls(&repo.root.join(DIR), &global_ids);
        }
        let has_code_index = idx.as_ref().is_some_and(|i| !i.files.is_empty());
        if has_code_index {
            if !core.is_empty() {
                core.push('\n');
                core.push_str(MAP_HINT);
            } else {
                core = MAP_HINT.to_string();
            }
            session.spent += text::approx_tokens(MAP_HINT) as u64;
        }
        parts.push(core);
        session.turn = CORE_GIVEN;
    }
    let last_msg = steps.iter().rposition(|s| matches!(s, Step::User { index, .. } | Step::Agent { index, .. } if *index > session.turn));
    if let Some(Step::User { index, text } | Step::Agent { index, text, .. }) = last_msg.map(|i| &steps[i]) {
        let (git_ts, _) = repo.sync_git_and_targets(&mut m);
        parts.push(hook::handle_prompt_with_index(&mut m, &v, &mut session, text, git_ts, idx.as_ref()));
        session.turn = *index;
    }
    // Files read or written in this turn (edits before its message still refresh activation).
    let from = last_msg.map(|i| i + 1).unwrap_or(0);
    for step in &steps[..from] {
        if let Step::Edit(path) = step {
            hook::handle_edit(&mut m, &hook::relativize_path(&repo.root, path));
        }
    }
    for step in &steps[from..] {
        match step {
            Step::Read(path) => {
                let rel = hook::relativize_path(&repo.root, path);
                parts.push(hook::read_context_with_index(&mut m, &v, &mut session, &rel, idx.as_ref()));
            }
            Step::Edit(path) => {
                let rel = hook::relativize_path(&repo.root, path);
                parts.push(hook::edit_context_with_index(&mut m, &v, &mut session, &rel, idx.as_ref()));
            }
            _ => {}
        }
    }

    let _ = session.save(&ses_dir);
    if lock.is_some() {
        let _ = repo.save(&m);
    }
    let text: Vec<String> = parts.into_iter().filter(|p| !p.trim().is_empty()).collect();
    Ok(cthg_core::antigravity::inject(&text.join("\n\n")))
}

/// `SessionState::turn` once the CORE was given and before any user message was handled.
const CORE_GIVEN: i64 = -2;
/// Rule files Antigravity loads by itself, and Claude Code: their imported memories are skipped.
const ANTIGRAVITY_NATIVE: &[&str] = &["AGENTS.md", "GEMINI.md"];
const CLAUDE_NATIVE: &[&str] = &["CLAUDE.md"];

fn run_hook(rest: &[String]) -> Result<(), String> {
    let event = rest.first().map(String::as_str).unwrap_or("");
    let mut raw_json = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw_json);
    if event == "antigravity" {
        // Antigravity always expects a JSON object on stdout, even when there is nothing to add.
        println!("{}", run_antigravity(&raw_json).unwrap_or_else(|_| "{}".into()));
        return Ok(());
    }
    if event == "antigravity-stop" {
        let p = cthg_core::antigravity::parse_payload(&raw_json);
        let _ = learn_corrections((!p.workspace.is_empty()).then(|| Path::new(&p.workspace)), &p.transcript);
        println!("{{}}");
        return Ok(());
    }
    let input = hook::HookInput::parse(&raw_json);
    let start = (!input.cwd.is_empty()).then(|| Path::new(&input.cwd));
    if event == "stop" {
        // Never blocks the stop and says nothing: what it learned shows up in later recalls.
        let _ = learn_corrections(start, &input.transcript_path);
        return Ok(());
    }
    let repo = Repo::open_at(start);
    // Busy memory: still answer, but read-only (the context matters more than the activation).
    let lock = repo.lock(HOOK_LOCK_WAIT);
    let mut m = repo.load()?;
    let ses_dir = repo.root.join(DIR).join(SES_DIR);
    let native = || CLAUDE_NATIVE.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    let save = |m: &Memory| {
        if lock.is_some() {
            let _ = repo.save(m);
        }
    };

    match event {
        "session-start" => {
            let idx = update_index_with_timeout(&repo.root, Duration::from_millis(500));
            let mut session = hook::SessionState::new(&input.session_id);
            session.native = native();
            let v = view(&repo, &m);
            let global = load_global();
            let (mut out, global_ids) = hook::handle_session_start_layered_with_global(&mut m, &v, &mut session, global.as_ref());
            if !global_ids.is_empty() {
                hook::record_global_recalls(&repo.root.join(DIR), &global_ids);
            }
            let has_code_index = idx.as_ref().is_some_and(|i| !i.files.is_empty());
            if has_code_index {
                if !out.is_empty() {
                    out.push('\n');
                    out.push_str(MAP_HINT);
                } else {
                    out = MAP_HINT.to_string();
                }
                session.spent += text::approx_tokens(MAP_HINT) as u64;
            }
            let _ = session.save(&ses_dir);
            save(&m);
            if !out.is_empty() {
                println!("{out}");
            }
        }
        "prompt" => {
            let idx = load_index_cached(&repo.root);
            let mut session = hook::SessionState::load(&ses_dir, &input.session_id);
            session.native = native();
            let (git_ts, moved) = repo.sync_git_and_targets(&mut m);
            let v = view(&repo, &m);
            let out = hook::handle_prompt_with_index(&mut m, &v, &mut session, &input.prompt, git_ts, idx.as_ref());
            if !out.is_empty() || moved > 0 {
                save(&m);
            }
            if !out.is_empty() {
                let _ = session.save(&ses_dir);
                println!("{out}");
            }
        }
        "read" => {
            let idx = load_index_cached(&repo.root);
            let rel = hook::relativize_path(&repo.root, &input.file_path);
            let mut session = hook::SessionState::load(&ses_dir, &input.session_id);
            session.native = native();
            let v = view(&repo, &m);
            let out = hook::handle_read_with_index(&mut m, &v, &mut session, &rel, idx.as_ref());
            if !out.is_empty() {
                let _ = session.save(&ses_dir);
                save(&m);
                println!("{out}");
            }
        }
        "edit" => {
            let idx = load_index_cached(&repo.root);
            let rel = hook::relativize_path(&repo.root, &input.file_path);
            let mut session = hook::SessionState::load(&ses_dir, &input.session_id);
            session.native = native();
            let v = view(&repo, &m);
            let before = m.nodes.iter().filter(|n| n.activation == 1.0).count();
            let context = hook::edit_context_with_index(&mut m, &v, &mut session, &rel, idx.as_ref());
            let after = m.nodes.iter().filter(|n| n.activation == 1.0).count();
            if !context.is_empty() || after != before || !rel.is_empty() {
                save(&m);
            }
            if !context.is_empty() {
                let _ = session.save(&ses_dir);
                println!("{}", hook::format_post_tool_use(&context));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Lockfiles and build output: never where a correction applies.
fn is_noise(rel: &str) -> bool {
    NOISE_DIRS.iter().any(|d| rel.starts_with(d)) || rel.rsplit('/').next().is_some_and(|f| NOISE.contains(&f))
}

/// End of a turn (`Stop` in Claude Code and in Antigravity): learns the user's corrections found
/// in the transcript (`cthg_core::correction`). Each one is learned once (`LEDGER`); if the memory
/// is busy, they wait for the next turn. Returns how many were learned.
fn learn_corrections(start: Option<&Path>, transcript: &str) -> Result<usize, String> {
    let repo = Repo::open_at(start);
    if !repo.file.is_file() || transcript.is_empty() {
        return Ok(0);
    }
    let raw = fs::read(transcript).map_err(|e| e.to_string())?;
    let conv = correction::parse(&String::from_utf8_lossy(&raw));
    let ledger = repo.root.join(DIR).join(LEDGER);
    let known: Vec<String> = fs::read_to_string(&ledger)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split('\t').next().map(str::to_string))
        .collect();
    let relative = |p: &str| {
        let rel = hook::relativize_path(&repo.root, p);
        if is_noise(&rel) {
            String::new()
        } else {
            rel
        }
    };
    let fresh: Vec<correction::Captured> = correction::captured(&conv, relative).into_iter().filter(|c| !known.contains(&c.key)).collect();
    if fresh.is_empty() {
        return Ok(0);
    }
    let Some(_lock) = repo.lock(HOOK_LOCK_WAIT) else { return Ok(0) };
    let mut m = repo.load()?;
    let (branch, commit) = (repo.branch(&m), repo.commit());
    let mut lines = String::new();
    for c in &fresh {
        let mut claim = c.claim.clone();
        claim.branch = branch.clone();
        claim.commit = commit.clone();
        correction::learn_correction(&mut m, &claim, now());
        lines.push_str(&format!("{}\t{}\n", c.key, text::clip(&c.correction, 60)));
    }
    repo.save(&m)?;
    use std::io::Write;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(&ledger).map_err(|e| e.to_string())?;
    f.write_all(lines.as_bytes()).map_err(|e| e.to_string())?;
    Ok(fresh.len())
}

/// `~/.gemini/antigravity*/brain/*/.system_generated/logs/transcript.jsonl` (CLI, IDE, 2.0).
fn antigravity_transcripts() -> Vec<PathBuf> {
    let Some(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok() else {
        return Vec::new();
    };
    let dirs = |p: &Path| -> Vec<PathBuf> {
        fs::read_dir(p).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default()
    };
    let mut out: Vec<PathBuf> = dirs(&Path::new(&home).join(".gemini"))
        .into_iter()
        .filter(|d| d.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("antigravity")))
        .flat_map(|d| dirs(&d.join("brain")))
        .map(|c| c.join(".system_generated").join("logs").join("transcript.jsonl"))
        .filter(|f| f.is_file())
        .collect();
    out.sort();
    out
}

/// A transcript, or every `.jsonl` under a folder (a Claude Code project folder, say). Subagent
/// transcripts are skipped: their prompts come from the main agent.
fn jsonl_under(p: &Path) -> Vec<PathBuf> {
    if p.is_file() {
        return vec![p.to_path_buf()];
    }
    let mut out = Vec::new();
    for e in fs::read_dir(p).into_iter().flatten().filter_map(|e| e.ok()) {
        let path = e.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) != Some("subagents") {
                out.extend(jsonl_under(&path));
            }
        } else if path.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Short id of a conversation: its Antigravity folder (`brain/<id>/…/transcript.jsonl`) or file name.
fn conversation_name(f: &Path) -> String {
    let name = if f.file_stem().and_then(|s| s.to_str()) == Some("transcript") {
        f.ancestors().nth(3).and_then(|d| d.file_name())
    } else {
        f.file_stem()
    };
    name.and_then(|n| n.to_str()).unwrap_or("?").chars().take(8).collect()
}

fn truncate_sig_map(sig: &str) -> String {
    if sig.is_empty() {
        String::new()
    } else {
        let mut s = if sig.starts_with('(') || sig.starts_with('<') || sig.starts_with(':') {
            sig.to_string()
        } else {
            format!(" {sig}")
        };
        if s.chars().count() > 100 {
            s = format!("{}…", s.chars().take(99).collect::<String>().trim_end());
        }
        s
    }
}

fn print_file_map_result(
    res: &index::FileMapResult,
    show_tests: bool,
    m_opt: Option<&Memory>,
    scores_opt: Option<&[score::Score]>,
) {
    println!("{}:", res.path);

    match res.kind {
        index::FileMapKind::NewFile => {
            println!("archivo nuevo: aún no está en el índice");
            if !res.direct_rules.is_empty() {
                if let (Some(m), Some(scores)) = (m_opt, scores_opt) {
                    println!("reglas:");
                    for id in &res.direct_rules {
                        let n = &m.nodes[*id as usize];
                        println!("  {}", hook::render_node(m, scores, n));
                    }
                }
            }
            if !res.siblings.is_empty() {
                println!("hermanos:");
                for sib in &res.siblings {
                    println!("  {}:", sib.path);
                    if sib.public_symbols.is_empty() {
                        println!("    (sin símbolos públicos)");
                    } else {
                        for s in &sib.public_symbols {
                            let parent_str = s.parent.as_ref().map(|p| format!(" [{p}]")).unwrap_or_default();
                            let sig_100 = truncate_sig_map(&s.signature);
                            println!("    pub {} {}{sig_100} (L{}-L{}){}", s.kind, s.name, s.line, s.end_line, parent_str);
                        }
                    }
                }
            }
        }
        index::FileMapKind::NotIndexed => {
            println!("no indexado");
            if !res.direct_rules.is_empty() {
                if let (Some(m), Some(scores)) = (m_opt, scores_opt) {
                    println!("reglas:");
                    for id in &res.direct_rules {
                        let n = &m.nodes[*id as usize];
                        println!("  {}", hook::render_node(m, scores, n));
                    }
                }
            }
        }
        index::FileMapKind::Indexed => {
            if !res.direct_rules.is_empty() {
                if let (Some(m), Some(scores)) = (m_opt, scores_opt) {
                    println!("reglas:");
                    for id in &res.direct_rules {
                        let n = &m.nodes[*id as usize];
                        println!("  {}", hook::render_node(m, scores, n));
                    }
                }
            }
            if !res.neighbor_rules.is_empty() {
                if let (Some(m), Some(scores)) = (m_opt, scores_opt) {
                    println!("en vecinos:");
                    for id in &res.neighbor_rules {
                        let n = &m.nodes[*id as usize];
                        println!("  {}", hook::render_node(m, scores, n));
                    }
                }
            }
            println!("importa:");
            if res.imports.is_empty() {
                println!("  (ninguna)");
            } else {
                for imp in &res.imports {
                    println!("  {imp}");
                }
            }
            println!("importado por:");
            if res.imported_by.is_empty() {
                println!("  (nadie)");
            } else {
                for imp_by in &res.imported_by {
                    println!("  {imp_by}");
                }
            }
            println!("símbolos:");
            let test_count = res.symbols.iter().filter(|s| s.test).count();
            let non_var_syms: Vec<_> = res
                .symbols
                .iter()
                .filter(|s| s.kind != "var" && (show_tests || !s.test))
                .collect();
            let var_syms: Vec<_> = res
                .symbols
                .iter()
                .filter(|s| s.kind == "var" && (show_tests || !s.test))
                .collect();

            let public_syms: Vec<_> = non_var_syms.iter().copied().filter(|s| s.public).collect();
            let private_syms: Vec<_> = non_var_syms.iter().copied().filter(|s| !s.public).collect();

            for s in public_syms {
                let parent_str = s.parent.as_ref().map(|p| format!(" [{p}]")).unwrap_or_default();
                let sig_100 = truncate_sig_map(&s.signature);
                println!("  pub {} {}{sig_100} (L{}-L{}){}", s.kind, s.name, s.line, s.end_line, parent_str);
            }
            for s in private_syms {
                let parent_str = s.parent.as_ref().map(|p| format!(" [{p}]")).unwrap_or_default();
                let sig_100 = truncate_sig_map(&s.signature);
                println!("  {} {}{sig_100} (L{}-L{}){}", s.kind, s.name, s.line, s.end_line, parent_str);
            }

            if !var_syms.is_empty() {
                if var_syms.len() > 10 && !show_tests {
                    println!("  {} variables de módulo (usa --tests para verlas)", var_syms.len());
                } else {
                    for s in &var_syms {
                        let pub_prefix = if s.public { "pub " } else { "" };
                        let parent_str = s.parent.as_ref().map(|p| format!(" [{p}]")).unwrap_or_default();
                        let sig_100 = truncate_sig_map(&s.signature);
                        println!("  {pub_prefix}{} {}{sig_100} (L{}-L{}){}", s.kind, s.name, s.line, s.end_line, parent_str);
                    }
                }
            }

            if test_count > 0 && !show_tests {
                println!("  {test_count} tests (usa --tests para verlos)");
            }

            if res.symbols.is_empty() {
                println!("  (ninguno)");
            }
        }
    }
}

fn walk_dir_rs(root: &Path, rel: &str, out: &mut Vec<String>) {
    let dir = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let child_rel = if rel.is_empty() {
            name_str.into_owned()
        } else {
            format!("{rel}/{name_str}")
        };
        let norm = spatial::normalize(&child_rel);
        if NOISE_DIRS.iter().any(|d| norm.starts_with(d) || norm.contains(d)) {
            continue;
        }
        if let Ok(ft) = entry.file_type() {
            if ft.is_dir() {
                walk_dir_rs(root, &norm, out);
            } else if ft.is_file() && cthg_core::lang::lang_of(&norm).is_some() {
                out.push(norm);
            }
        }
    }
}

fn load_index_cached(root: &Path) -> Option<index::ProjectIndex> {
    let p = root.join(DIR).join("indice.tsv");
    let content = fs::read_to_string(p).ok()?;
    index::parse_index(&content).ok()
}

fn update_index_with_timeout(root: &Path, timeout: Duration) -> Option<index::ProjectIndex> {
    let root_buf = root.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::spawn(move || {
        let res = run_index_update(&root_buf, false);
        let _ = tx.send(res);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok((idx, _))) => Some(idx),
        _ => load_index_cached(root),
    }
}

fn run_index_update(root: &Path, rebuild: bool) -> Result<(index::ProjectIndex, index::IndexStats), String> {
    let list_files_fn = || {
        if let Some(rf) = repo_files(root) {
            rf.files().to_vec()
        } else {
            let mut out = Vec::new();
            walk_dir_rs(root, "", &mut out);
            out
        }
    };
    let file_meta_fn = |p: &str| {
        let meta = fs::metadata(root.join(p)).ok()?;
        let size = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Some((size, mtime))
    };
    let read_file_bytes_fn = |p: &str| fs::read(root.join(p)).ok();
    let read_index_fn = || fs::read_to_string(root.join(DIR).join("indice.tsv")).ok();
    let write_index_fn = |content: &str| {
        let dir = root.join(DIR);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let gi = dir.join(".gitignore");
        if gi.exists() {
            if let Ok(existing) = fs::read_to_string(&gi) {
                if !existing.lines().any(|l| l.trim() == "indice.tsv" || l.trim() == "indice.tsv*") {
                    let mut updated = existing;
                    if !updated.ends_with('\n') {
                        updated.push('\n');
                    }
                    updated.push_str("indice.tsv\n");
                    let _ = fs::write(&gi, updated);
                }
            }
        } else {
            let _ = fs::write(&gi, "indice.tsv\n");
        }

        let pid = std::process::id();
        let tmp_path = dir.join(format!("indice.tsv.{pid}.tmp"));
        let final_path = dir.join("indice.tsv");
        fs::write(&tmp_path, content).map_err(|e| format!("no se pudo escribir temporal de índice: {e}"))?;
        if let Err(e) = fs::rename(&tmp_path, &final_path) {
            let _ = fs::remove_file(&final_path);
            if let Err(e2) = fs::rename(&tmp_path, &final_path) {
                let _ = fs::remove_file(&tmp_path);
                return Err(format!("no se pudo renombrar temporal de índice: {e2} (anterior: {e})"));
            }
        }
        Ok(())
    };

    let probe = index::IndexProbe {
        list_files: &list_files_fn,
        file_meta: &file_meta_fn,
        read_file_bytes: &read_file_bytes_fn,
        read_index: &read_index_fn,
        write_index: &write_index_fn,
    };

    index::update_index(&probe, rebuild)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_global_folder_is_not_a_project() {
        let base = std::env::temp_dir().join(format!("cthg-root-{}", std::process::id()));
        let home = base.join("home");
        let proj = home.join("proyecto");
        fs::create_dir_all(home.join(DIR)).unwrap();
        fs::write(home.join(DIR).join("global.cthg"), b"x").unwrap();
        fs::create_dir_all(&proj).unwrap();
        // No project memory yet: the home `.cthg/` (global only) must not be picked.
        assert_ne!(find_root_from(Some(&proj)), home);
        fs::create_dir_all(proj.join(DIR)).unwrap();
        fs::write(proj.join(DIR).join(FILE), b"x").unwrap();
        assert_eq!(find_root_from(Some(&proj.join(DIR))), proj);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn antigravity_write_to_file_without_read_injects_file_rules_and_agent_message_triggers_prompt_without_correction() {
        let base = std::env::temp_dir().join(format!("cthg-agy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        let mut m = Memory::default();
        if let Some(b) = git(&base, &["rev-parse", "--abbrev-ref", "HEAD"]) {
            m.set_meta("trunk", b.trim());
        }
        // Make memory larger than WHOLE_MAX so pathed rules are not injected in core_context.
        for i in 0..25 {
            let mut c = Claim::rule(&format!("Regla auxiliar número {i} con varias palabras para superar el umbral"), &format!("src/other{i}.rs"));
            c.origin = Origin::Imported;
            learn::learn(&mut m, &c, 10.0 + i as f64);
        }
        let mut id_rule = Claim::rule("En src/id.rs los ids son base36 de 4 caracteres en minúscula", "src/id.rs");
        id_rule.origin = Origin::Imported;
        let id_node = learn::learn(&mut m, &id_rule, 100.0)[0].node;
        let mut st_rule = Claim::rule("En src/storage.rs las tareas se guardan en tareas.tsv", "src/storage.rs");
        st_rule.origin = Origin::Imported;
        let st_node = learn::learn(&mut m, &st_rule, 101.0)[0].node;
        binary::save(&m, &base.join(DIR).join(FILE)).unwrap();

        // 1. Transcript with only `write_to_file` on `src/id.rs` produces the edit injection.
        let t_write = base.join("write_only.jsonl");
        let target_id = base.join("src").join("id.rs");
        let line_write = format!(
            "{{\"step_index\":0,\"source\":\"MODEL\",\"type\":\"PLANNER_RESPONSE\",\"status\":\"DONE\",\"tool_calls\":[{{\"name\":\"write_to_file\",\"args\":{{\"TargetFile\":\"{}\",\"CodeContent\":\"x\"}}}}]}}\n",
            hook::escape_json(&target_id.to_string_lossy())
        );
        fs::write(&t_write, &line_write).unwrap();
        let payload_write = format!(
            "{{\"conversationId\":\"conv-write\",\"workspacePaths\":[\"{}\"],\"transcriptPath\":\"{}\"}}",
            hook::escape_json(&base.to_string_lossy()),
            hook::escape_json(&t_write.to_string_lossy())
        );
        let out_write = run_antigravity(&payload_write).unwrap();
        assert!(out_write.contains("src/id.rs, que acabas de escribir"), "{out_write}");
        assert!(out_write.contains("base36 de 4 caracteres"), "{out_write}");
        let ses_write = hook::SessionState::load(&base.join(DIR).join(SES_DIR), "conv-write");
        assert!(ses_write.was_injected(id_node));

        // 2. Inter-agent `SYSTEM_MESSAGE` activates the message channel and is never a user correction.
        let t_agent = base.join("agent_msg.jsonl");
        let lines_agent = concat!(
            r#"{"step_index":1,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","created_at":"2026-10-18T03:26:12Z","content":"The following is a <SYSTEM_MESSAGE> not actually sent by the user. It is provided by the system as important information to pay attention to.\n\n<SYSTEM_MESSAGE>\n[Message] timestamp=2026-10-18T03:26:12Z sender=580c7845-203e-44db-a9d9-f449680aa27d priority=MESSAGE_PRIORITY_HIGH content=Construye `src/storage.rs` cumpliendo la interfaz.\n</SYSTEM_MESSAGE>"}"#,
            "\n",
            r#"{"step_index":2,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","created_at":"2026-10-18T03:27:25Z","content":"The following is a <SYSTEM_MESSAGE> not actually sent by the user. It is provided by the system as important information to pay attention to.\n\n<SYSTEM_MESSAGE>\n[Message] timestamp=2026-10-18T03:27:24Z sender=580c7845-203e-44db-a9d9-f449680aa27d priority=MESSAGE_PRIORITY_HIGH content=no, rompiste todo, arréglalo de nuevo en src/storage.rs\n</SYSTEM_MESSAGE>"}"#,
            "\n",
        );
        fs::write(&t_agent, lines_agent).unwrap();
        let payload_agent = format!(
            "{{\"conversationId\":\"conv-agent\",\"workspacePaths\":[\"{}\"],\"transcriptPath\":\"{}\"}}",
            hook::escape_json(&base.to_string_lossy()),
            hook::escape_json(&t_agent.to_string_lossy())
        );
        let out_agent = run_antigravity(&payload_agent).unwrap();
        assert!(out_agent.contains("tareas.tsv"), "agent message triggered lexical recall for src/storage.rs: {out_agent}");
        let ses_agent = hook::SessionState::load(&base.join(DIR).join(SES_DIR), "conv-agent");
        assert!(ses_agent.was_injected(st_node));
        assert_eq!(ses_agent.turn, 2);

        // Neither `correction::parse` nor `learn_corrections` treats agent messages as user turns or corrections.
        let conv = correction::parse(lines_agent);
        assert!(conv.turns.is_empty());
        assert!(correction::corrections(&conv).is_empty());
        assert_eq!(learn_corrections(Some(&base), &t_agent.to_string_lossy()).unwrap(), 0);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn recall_with_neighbors_and_sin_vecinos_flag() {
        let base = std::env::temp_dir().join(format!("cthg-recall-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(DIR)).unwrap();

        let mut m = Memory::default();
        let mut cli_rule = Claim::rule("En src/cli.rs los codigos de salida son 2", "src/cli.rs");
        cli_rule.origin = Origin::Imported;
        learn::learn(&mut m, &cli_rule, 100.0);
        binary::save(&m, &base.join(DIR).join(FILE)).unwrap();

        // Create indice.tsv where src/main.rs imports src/cli.rs
        let mut index = index::ProjectIndex::default();
        index.files.push(index::FileEntry {
            path: "src/main.rs".to_string(),
            lang: "rust".to_string(),
            size: 10,
            mtime: 1,
            hash: 1,
                    status: "-".to_string(),
            symbols: vec![],
            imports: vec![cthg_core::lang_rust::Import {
                raw: "use crate::cli;".to_string(),
                resolved: Some("src/cli.rs".to_string()),
            }],
            calls: vec![],
        });
        index.files.push(index::FileEntry {
            path: "src/cli.rs".to_string(),
            lang: "rust".to_string(),
            size: 10,
            mtime: 1,
            hash: 2,
                    status: "-".to_string(),
            symbols: vec![],
            imports: vec![],
            calls: vec![],
        });
        let index_content = index::format_index(&index);
        fs::write(base.join(DIR).join("indice.tsv"), index_content).unwrap();

        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&base).unwrap();

        let repo = Repo::open();
        let idx = load_index_cached(&repo.root);
        assert!(idx.is_some());
        let targets = vec![Target::new("src/main.rs", 1.0)];
        let expanded = spatial::expand_neighbors(&targets, idx.as_ref(), spatial::W_NEIGHBOR, spatial::MAX_NEIGHBORS);
        assert!(expanded.iter().any(|t| t.path == "src/cli.rs"));

        let origins = spatial::node_neighbor_origins(&m, &expanded, &[0]);
        assert_eq!(origins.get(&0).map(String::as_str), Some("src/main.rs"));

        std::env::set_current_dir(prev).unwrap();
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_hook_map_hint_and_silent_panic() {
        let base = std::env::temp_dir().join(format!("cthg-hint-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(DIR)).unwrap();

        let mut m = Memory::default();
        let rule = Claim::rule("Regla base de prueba", "src/lib.rs");
        learn::learn(&mut m, &rule, 100.0);
        binary::save(&m, &base.join(DIR).join(FILE)).unwrap();

        // 1. Without index: antigravity first turn does not contain MAP_HINT
        let t_file = base.join("transcript.jsonl");
        fs::write(&t_file, "").unwrap();
        let payload1 = format!(
            "{{\"conversationId\":\"conv-hint\",\"workspacePaths\":[\"{}\"],\"transcriptPath\":\"{}\"}}",
            hook::escape_json(&base.to_string_lossy()),
            hook::escape_json(&t_file.to_string_lossy())
        );
        let out1 = run_antigravity(&payload1).unwrap();
        assert!(!out1.contains("Mapa del proyecto:"), "Should not contain hint without index: {out1}");

        // Now create a real source file and an index with files
        fs::create_dir_all(base.join("src")).unwrap();
        fs::write(base.join("src").join("lib.rs"), "pub fn hello() {}").unwrap();
        let mut index = index::ProjectIndex::default();
        index.files.push(index::FileEntry {
            path: "src/lib.rs".to_string(),
            lang: "rust".to_string(),
            size: 10,
            mtime: 1,
            hash: 1,
            status: "-".to_string(),
            symbols: vec![],
            imports: vec![],
            calls: vec![],
        });
        fs::write(base.join(DIR).join("indice.tsv"), index::format_index(&index)).unwrap();

        // 2. With index on fresh conversation: contains MAP_HINT
        let payload2 = format!(
            "{{\"conversationId\":\"conv-hint-2\",\"workspacePaths\":[\"{}\"],\"transcriptPath\":\"{}\"}}",
            hook::escape_json(&base.to_string_lossy()),
            hook::escape_json(&t_file.to_string_lossy())
        );
        let out2 = run_antigravity(&payload2).unwrap();
        assert!(out2.contains("Mapa del proyecto:"), "Should contain hint with index: {out2}");

        // 3. Second call in same conversation: does not repeat MAP_HINT
        let out3 = run_antigravity(&payload2).unwrap();
        assert!(!out3.contains("Mapa del proyecto:"), "Should not repeat hint in second turn: {out3}");

        // 4. Verify silent panic hook behavior
        std::panic::set_hook(Box::new(|_| {}));
        let res = std::panic::catch_unwind(|| {
            panic!("test silent panic in hook execution");
        });
        assert!(res.is_err(), "panic should be caught without aborting");

        let _ = fs::remove_dir_all(&base);
    }

}
