//! Code graph derived index (`.cthg/indice.tsv`).
//!
//! Extracts symbols, imports, and calls from the repository to automate spatial navigation
//! without cluttering the causal memory (`memoria.cthg`) or inflating context token budgets.

use crate::binary::crc32;
pub use crate::lang_rust::resolve_import;
use crate::lang::{self, Call, Import, Symbol};
use crate::model::*;
use crate::score::Score;
use crate::spatial;
use std::collections::{HashMap, HashSet};

pub const INDEX_HEADER: &str = "cthg-indice 2";
pub const MAX_INDEXABLE_SIZE: u64 = 512 * 1024; // 512 KB

/// A single indexed source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub lang: String,
    pub size: u64,
    pub mtime: u64,
    pub hash: u32,
    pub status: String,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<Import>,
    pub calls: Vec<Call>,
}

/// The entire project code index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectIndex {
    pub files: Vec<FileEntry>,
}

impl ProjectIndex {
    pub fn get_file(&self, path: &str) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.path == path)
    }

    pub fn total_symbols(&self) -> usize {
        self.files.iter().map(|f| f.symbols.len()).sum()
    }

    pub fn total_imports(&self) -> usize {
        self.files.iter().map(|f| f.imports.len()).sum()
    }

    pub fn resolved_imports(&self) -> usize {
        self.files
            .iter()
            .flat_map(|f| &f.imports)
            .filter(|i| i.resolved.is_some())
            .count()
    }
}

/// Callbacks to inspect and update the index without coupling `cthg-core` directly to disk I/O.
pub struct IndexProbe<'a> {
    pub list_files: &'a dyn Fn() -> Vec<String>,
    pub file_meta: &'a dyn Fn(&str) -> Option<(u64, u64)>, // (size, mtime)
    pub read_file_bytes: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    pub read_index: &'a dyn Fn() -> Option<String>,
    pub write_index: &'a dyn Fn(&str) -> Result<(), String>,
}

/// Statistics returned by an incremental index run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexStats {
    pub total_files: usize,
    pub rescanned: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub total_symbols: usize,
    pub resolved_imports: usize,
    pub total_imports: usize,
    pub failed_files: Vec<String>,
}

/// Parses the TSV index format.
pub fn parse_index(text: &str) -> Result<ProjectIndex, String> {
    let mut lines = text.lines();
    let header = lines.next().ok_or_else(|| "índice vacío".to_string())?;
    if header.trim() != INDEX_HEADER {
        return Err(format!("versión o cabecera de índice desconocida: '{header}'"));
    }

    let mut files: Vec<FileEntry> = Vec::new();

    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.first().copied() {
            Some("F") => {
                if parts.len() < 6 {
                    return Err(format!("línea F incompleta: '{line}'"));
                }
                let path = parts[1].to_string();
                let lang = parts[2].to_string();
                let size = parts[3].parse().map_err(|_| "tamaño inválido en línea F")?;
                let mtime = parts[4].parse().map_err(|_| "mtime inválido en línea F")?;
                let hash = u32::from_str_radix(parts[5], 16).map_err(|_| "hash inválido en línea F")?;
                let status = parts.get(6).copied().unwrap_or("-").to_string();
                files.push(FileEntry {
                    path,
                    lang,
                    size,
                    mtime,
                    hash,
                    status,
                    symbols: Vec::new(),
                    imports: Vec::new(),
                    calls: Vec::new(),
                });
            }
            Some("S") => {
                let Some(cur) = files.last_mut() else {
                    return Err("línea S sin archivo previo".into());
                };
                if parts.len() < 8 {
                    return Err(format!("línea S incompleta: '{line}'"));
                }
                let kind = parts[1].to_string();
                let name = parts[2].to_string();
                let start_line = parts[3].parse().map_err(|_| "línea inválida en S")?;
                let end_line = parts[4].parse().map_err(|_| "línea fin inválida en S")?;
                let public = parts[5] == "1";
                let parent = if parts[6] == "-" { None } else { Some(parts[6].to_string()) };
                let test = parts[7] == "1";
                let signature = parts
                    .get(8)
                    .map(|s| if *s == "-" { "" } else { *s })
                    .unwrap_or("")
                    .to_string();
                let decorators = parts
                    .get(9)
                    .map(|s| {
                        if *s == "-" || s.is_empty() {
                            Vec::new()
                        } else {
                            s.split(';').map(str::to_string).collect()
                        }
                    })
                    .unwrap_or_default();
                cur.symbols.push(Symbol {
                    name,
                    kind,
                    line: start_line,
                    end_line,
                    public,
                    parent,
                    test,
                    signature,
                    decorators,
                });
            }
            Some("I") => {
                let Some(cur) = files.last_mut() else {
                    return Err("línea I sin archivo previo".into());
                };
                if parts.len() < 3 {
                    return Err(format!("línea I incompleta: '{line}'"));
                }
                let raw = parts[1].to_string();
                let resolved = if parts[2] == "-" { None } else { Some(parts[2].to_string()) };
                cur.imports.push(Import { raw, resolved });
            }
            Some("C") => {
                let Some(cur) = files.last_mut() else {
                    return Err("línea C sin archivo previo".into());
                };
                if parts.len() < 4 {
                    return Err(format!("línea C incompleta: '{line}'"));
                }
                let from = parts[1].to_string();
                let name = parts[2].to_string();
                let line_num = parts[3].parse().map_err(|_| "línea inválida en C")?;
                cur.calls.push(Call {
                    from,
                    name,
                    line: line_num,
                });
            }
            _ => {}
        }
    }

    Ok(ProjectIndex { files })
}

/// Serializes the project index into TSV format.
pub fn format_index(index: &ProjectIndex) -> String {
    let mut out = String::new();
    out.push_str(INDEX_HEADER);
    out.push('\n');

    for file in &index.files {
        let status = if file.status.is_empty() { "-" } else { &file.status };
        out.push_str(&format!(
            "F\t{}\t{}\t{}\t{}\t{:08x}\t{}\n",
            file.path, file.lang, file.size, file.mtime, file.hash, status
        ));
        for s in &file.symbols {
            let pub_str = if s.public { "1" } else { "0" };
            let parent_str = s.parent.as_deref().unwrap_or("-");
            let test_str = if s.test { "1" } else { "0" };
            let sig_str = if s.signature.is_empty() { "-" } else { &s.signature };
            let dec_str = if s.decorators.is_empty() { "-" } else { &s.decorators.join(";") };
            out.push_str(&format!(
                "S\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                s.kind, s.name, s.line, s.end_line, pub_str, parent_str, test_str, sig_str, dec_str
            ));
        }
        for i in &file.imports {
            let res_str = i.resolved.as_deref().unwrap_or("-");
            out.push_str(&format!("I\t{}\t{}\n", i.raw, res_str));
        }
        for c in &file.calls {
            out.push_str(&format!("C\t{}\t{}\t{}\n", c.from, c.name, c.line));
        }
    }

    out
}

/// Performs an incremental update of the index against repository files.
pub fn update_index(probe: &IndexProbe, rebuild: bool) -> Result<(ProjectIndex, IndexStats), String> {
    let mut existing: ProjectIndex = if rebuild {
        ProjectIndex::default()
    } else {
        (probe.read_index)()
            .and_then(|text| parse_index(&text).ok())
            .unwrap_or_default()
    };

    let all_files = (probe.list_files)();
    let mut candidate_files: Vec<String> = all_files
        .into_iter()
        .filter(|p| lang::lang_of(p).is_some() && !is_noise(p))
        .collect();
    candidate_files.sort();

    let target_set: HashSet<String> = candidate_files.iter().cloned().collect();
    let old_count = existing.files.len();
    existing.files.retain(|f| target_set.contains(&f.path));
    let removed = old_count.saturating_sub(existing.files.len());

    let mut existing_map: HashMap<String, FileEntry> = existing
        .files
        .into_iter()
        .map(|f| (f.path.clone(), f))
        .collect();

    let mut rescanned = 0usize;
    let mut unchanged = 0usize;
    let mut failed_files = Vec::new();
    let mut new_files = Vec::new();

    for path in candidate_files {
        let Some((size, mtime)) = (probe.file_meta)(&path) else {
            continue;
        };

        if size > MAX_INDEXABLE_SIZE {
            continue;
        }

        let Some(file_lang) = lang::lang_of(&path) else {
            continue;
        };

        if let Some(entry) = existing_map.remove(&path) {
            if entry.size == size && entry.mtime == mtime {
                if entry.status == "error" {
                    failed_files.push(entry.path.clone());
                }
                new_files.push(entry);
                unchanged += 1;
                continue;
            }

            // mtime or size changed: check hash before rescanning
            let Some(bytes) = (probe.read_file_bytes)(&path) else {
                continue;
            };

            // Bundle detection: skip if average line length > 500 chars
            let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
            if bytes.len() / (newlines + 1) > 500 {
                continue;
            }

            let hash = crc32(&bytes);
            if entry.hash == hash {
                if entry.status == "error" {
                    failed_files.push(entry.path.clone());
                }
                new_files.push(FileEntry {
                    size,
                    mtime,
                    ..entry
                });
                unchanged += 1;
                continue;
            }

            // Rescan with catch_unwind
            let scan_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lang::scan(file_lang, &bytes)
            }));
            let (scan, status) = match scan_res {
                Ok(s) => (s, "-".to_string()),
                Err(_) => {
                    failed_files.push(path.clone());
                    (lang::FileScan::default(), "error".to_string())
                }
            };
            new_files.push(FileEntry {
                path,
                lang: file_lang.as_str().to_string(),
                size,
                mtime,
                hash,
                status,
                symbols: scan.symbols,
                imports: scan.imports,
                calls: scan.calls,
            });
            rescanned += 1;
        } else {
            // New file
            let Some(bytes) = (probe.read_file_bytes)(&path) else {
                continue;
            };
            // Bundle detection: skip if average line length > 500 chars
            let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
            if bytes.len() / (newlines + 1) > 500 {
                continue;
            }

            let hash = crc32(&bytes);
            let scan_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lang::scan(file_lang, &bytes)
            }));
            let (scan, status) = match scan_res {
                Ok(s) => (s, "-".to_string()),
                Err(_) => {
                    failed_files.push(path.clone());
                    (lang::FileScan::default(), "error".to_string())
                }
            };
            new_files.push(FileEntry {
                path,
                lang: file_lang.as_str().to_string(),
                size,
                mtime,
                hash,
                status,
                symbols: scan.symbols,
                imports: scan.imports,
                calls: scan.calls,
            });
            rescanned += 1;
        }
    }

    // Resolve imports across all files in the index
    resolve_all_imports(&mut new_files, probe);

    let final_index = ProjectIndex { files: new_files };
    let stats = IndexStats {
        total_files: final_index.files.len(),
        rescanned,
        unchanged,
        removed,
        total_symbols: final_index.total_symbols(),
        resolved_imports: final_index.resolved_imports(),
        total_imports: final_index.total_imports(),
        failed_files,
    };

    let formatted = format_index(&final_index);
    (probe.write_index)(&formatted)?;

    Ok((final_index, stats))
}

pub fn is_noise(p: &str) -> bool {
    p.contains("target/")
        || p.contains("node_modules/")
        || p.contains("dist/")
        || p.contains("build/")
        || p.contains(".cthg/")
        || p.contains("__pycache__/")
        || p.contains(".venv/")
        || p.contains("venv/")
        || p.contains("env/")
        || p.contains(".next/")
        || p.contains("coverage/")
        || p.contains(".mypy_cache/")
        || p.contains(".pytest_cache/")
        || p.ends_with(".min.js")
}

/// Resolves imports across all indexed files.
fn resolve_all_imports(files: &mut [FileEntry], probe: &IndexProbe) {
    let all_paths: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    let workspace_crates = discover_workspace_crates(probe);

    for file in files.iter_mut() {
        let file_path = file.path.clone();
        let file_lang = lang::Lang::from_name(&file.lang).unwrap_or(lang::Lang::Rust);
        for import in &mut file.imports {
            import.resolved = lang::resolve_import(file_lang, &file_path, &import.raw, &all_paths, &workspace_crates);
        }
    }
}

/// Discovers `(crate_ident, src_dir)` mappings by reading Cargo.toml files.
fn discover_workspace_crates(probe: &IndexProbe) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let files = (probe.list_files)();
    for p in files {
        if p.ends_with("Cargo.toml") && !is_noise(&p) {
            if let Some(bytes) = (probe.read_file_bytes)(&p) {
                let text = String::from_utf8_lossy(&bytes);
                if let Some(name) = extract_cargo_package_name(&text) {
                    let ident = name.replace('-', "_");
                    let dir = if let Some(parent) = p.strip_suffix("Cargo.toml") {
                        let trimmed = parent.trim_end_matches('/');
                        if trimmed.is_empty() {
                            "src".to_string()
                        } else {
                            format!("{trimmed}/src")
                        }
                    } else {
                        "src".to_string()
                    };
                    map.insert(ident, dir);
                }
            }
        }
    }
    map
}

fn extract_cargo_package_name(toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_package = line == "[package]";
            continue;
        }
        if in_package && line.starts_with("name") {
            if let Some((_, val)) = line.split_once('=') {
                let name = val.trim().trim_matches('"').trim_matches('\'').trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}



// ---------------------------------------------------------------------------
// Query APIs
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhereResult {
    pub path: String,
    pub line: usize,
    pub kind: String,
    pub name: String,
    pub parent: Option<String>,
    pub signature: String,
}

/// Finds definitions of `query` (supports `name` or `Parent::name`).
pub fn where_symbol(index: &ProjectIndex, query: &str) -> Vec<WhereResult> {
    let (target_parent, target_name) = if let Some((p, n)) = query.split_once("::") {
        (Some(p), n)
    } else {
        (None, query)
    };

    let mut results = Vec::new();
    for file in &index.files {
        for s in &file.symbols {
            if s.name == target_name {
                if let Some(req_p) = target_parent {
                    if s.parent.as_deref() == Some(req_p) {
                        results.push(WhereResult {
                            path: file.path.clone(),
                            line: s.line,
                            kind: s.kind.clone(),
                            name: s.name.clone(),
                            parent: s.parent.clone(),
                            signature: s.signature.clone(),
                        });
                    }
                } else {
                    results.push(WhereResult {
                        path: file.path.clone(),
                        line: s.line,
                        kind: s.kind.clone(),
                        name: s.name.clone(),
                        parent: s.parent.clone(),
                        signature: s.signature.clone(),
                    });
                }
            }
        }
    }
    results.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
    results
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallerItem {
    pub path: String,
    pub line: usize,
    pub from: String,
    pub is_test: bool,
    pub is_prioritized: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallersResult {
    pub callers: Vec<CallerItem>,
    pub is_ambiguous: bool,
    pub def_files_count: usize,
}

/// Finds all callers to `query`.
pub fn callers(index: &ProjectIndex, query: &str) -> CallersResult {
    let (_, target_name) = if let Some((_, n)) = query.split_once("::") {
        (Some(()), n)
    } else {
        (None, query)
    };

    // 1. Find defining files and their language families
    let def_files: HashSet<String> = index
        .files
        .iter()
        .filter(|f| f.symbols.iter().any(|s| s.name == target_name))
        .map(|f| f.path.clone())
        .collect();

    let def_families: HashSet<lang::LangFamily> = index
        .files
        .iter()
        .filter(|f| def_files.contains(&f.path))
        .filter_map(|f| lang::Lang::from_name(&f.lang).map(|l| l.family()))
        .collect();

    let def_count = def_files.len();
    let is_ambiguous = def_count > 3;

    // 2. Importers of the defining files
    let mut importing_files: HashSet<String> = def_files.clone();
    for f in &index.files {
        for imp in &f.imports {
            if let Some(target) = &imp.resolved {
                if def_files.contains(target) {
                    importing_files.insert(f.path.clone());
                    break;
                }
            }
        }
    }

    // 3. Collect calls, strictly respecting language family boundaries
    let mut callers = Vec::new();
    for file in &index.files {
        let file_fam = lang::Lang::from_name(&file.lang)
            .map(|l| l.family())
            .unwrap_or(lang::LangFamily::Rust);
        if !def_families.contains(&file_fam) {
            continue; // Cannot call symbols from a different language family
        }
        let file_is_test = file.path.contains("tests/")
            || file.path.ends_with("_test.rs")
            || file.path.ends_with("_test.py")
            || file.path.contains(".test.")
            || file.path.contains(".spec.");

        for call in &file.calls {
            if call.name == target_name {
                let is_test = file_is_test
                    || file
                        .symbols
                        .iter()
                        .any(|s| s.test && (s.name == call.from || call.from.ends_with(&format!("::{}", s.name))));
                let is_prioritized = importing_files.contains(&file.path);
                callers.push(CallerItem {
                    path: file.path.clone(),
                    line: call.line,
                    from: call.from.clone(),
                    is_test,
                    is_prioritized,
                });
            }
        }
    }

    // Sort: prioritized first, then path, then line
    callers.sort_by(|a, b| {
        b.is_prioritized
            .cmp(&a.is_prioritized)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });

    CallersResult {
        callers,
        is_ambiguous,
        def_files_count: def_count,
    }
}

/// Returns `(imports, imported_by)` for a given file.
pub fn file_neighbors(index: &ProjectIndex, path: &str) -> (Vec<String>, Vec<String>) {
    let mut imports = Vec::new();
    if let Some(entry) = index.get_file(path) {
        for imp in &entry.imports {
            if let Some(res) = &imp.resolved {
                if res != path && !imports.contains(res) {
                    imports.push(res.clone());
                }
            }
        }
    }

    let mut imported_by = Vec::new();
    for file in &index.files {
        if file.path == path {
            continue;
        }
        if file.imports.iter().any(|i| i.resolved.as_deref() == Some(path))
            && !imported_by.contains(&file.path)
        {
            imported_by.push(file.path.clone());
        }
    }

    imports.sort();
    imported_by.sort();
    (imports, imported_by)
}

#[derive(Clone, Debug)]
pub struct TreeFileSummary {
    pub path: String,
    pub total_symbols: usize,
    pub public_symbols: Vec<Symbol>,
}

/// Generates a compact tree representation of files in a directory or whole repo.
pub fn compact_tree(index: &ProjectIndex, prefix: &str, limit: usize) -> Vec<TreeFileSummary> {
    let mut out = Vec::new();
    let norm_prefix = prefix.trim_matches('/');

    for file in &index.files {
        if !norm_prefix.is_empty() && !spatial::is_ancestor_or_self(norm_prefix, &file.path) {
            continue;
        }
        let public_symbols: Vec<Symbol> = file.symbols.iter().filter(|s| s.public).cloned().collect();
        out.push(TreeFileSummary {
            path: file.path.clone(),
            total_symbols: file.symbols.len(),
            public_symbols,
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Memory integration (separate functions for 8a/8b reuse)
// ---------------------------------------------------------------------------

/// Finds IDs of rules anchored directly at `path` or one of its ancestor directories.
pub fn anchored_rules_for_file(m: &Memory, scores: &[Score], path: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    for (i, node) in m.nodes.iter().enumerate() {
        if node.path.is_empty() || !spatial::is_ancestor_or_self(&node.path, path) {
            continue;
        }
        if is_injectable_rule(node, scores.get(i)) {
            ids.push(node.id);
        }
    }
    ids
}

/// Finds IDs of rules anchored at neighbor files or their ancestor folders,
/// excluding rules already found directly for the target file.
pub fn neighbor_rules_for_file(
    m: &Memory,
    scores: &[Score],
    neighbors: &[String],
    exclude_ids: &[u32],
) -> Vec<u32> {
    let exclude_set: HashSet<u32> = exclude_ids.iter().copied().collect();
    let mut ids = Vec::new();

    for (i, node) in m.nodes.iter().enumerate() {
        if node.path.is_empty() || exclude_set.contains(&node.id) {
            continue;
        }
        let matches_neighbor = neighbors
            .iter()
            .any(|neigh| spatial::is_ancestor_or_self(&node.path, neigh));
        if matches_neighbor && is_injectable_rule(node, scores.get(i)) {
            ids.push(node.id);
        }
    }
    ids
}

fn is_injectable_rule(node: &Node, score_opt: Option<&Score>) -> bool {
    if node.has(flags::POTENTIAL) || node.has(flags::STALE) || node.has(flags::PROPOSED) {
        return false;
    }
    if let Some(score) = score_opt {
        if !score.visible {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileMapKind {
    Indexed,
    NotIndexed,
    NewFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMapSibling {
    pub path: String,
    pub public_symbols: Vec<Symbol>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMapResult {
    pub path: String,
    pub kind: FileMapKind,
    pub direct_rules: Vec<u32>,
    pub neighbor_rules: Vec<u32>,
    pub imports: Vec<String>,
    pub imported_by: Vec<String>,
    pub symbols: Vec<Symbol>,
    pub siblings: Vec<FileMapSibling>,
}

/// Computes the structured map for a given target path according to its presence in the index
/// and on disk, ordering direct rules, neighbor rules, dependencies, and symbols.
pub fn file_map(
    idx: &ProjectIndex,
    m: Option<&Memory>,
    scores: &[Score],
    target_path: &str,
    exists_on_disk: bool,
) -> FileMapResult {
    let norm = spatial::normalize(target_path);
    let empty_scores = Vec::new();
    let actual_scores = if scores.is_empty() {
        &empty_scores[..]
    } else {
        scores
    };

    if let Some(entry) = idx.get_file(&norm) {
        let direct_rules = m.map(|mem| anchored_rules_for_file(mem, actual_scores, &norm)).unwrap_or_default();
        let (imports, imported_by) = file_neighbors(idx, &norm);
        let mut all_neighbors = imports.clone();
        all_neighbors.extend(imported_by.clone());
        let neighbor_rules = m.map(|mem| neighbor_rules_for_file(mem, actual_scores, &all_neighbors, &direct_rules)).unwrap_or_default();

        FileMapResult {
            path: norm,
            kind: FileMapKind::Indexed,
            direct_rules,
            neighbor_rules,
            imports,
            imported_by,
            symbols: entry.symbols.clone(),
            siblings: Vec::new(),
        }
    } else if exists_on_disk {
        let direct_rules = m.map(|mem| anchored_rules_for_file(mem, actual_scores, &norm)).unwrap_or_default();
        FileMapResult {
            path: norm,
            kind: FileMapKind::NotIndexed,
            direct_rules,
            neighbor_rules: Vec::new(),
            imports: Vec::new(),
            imported_by: Vec::new(),
            symbols: Vec::new(),
            siblings: Vec::new(),
        }
    } else {
        let direct_rules = m.map(|mem| anchored_rules_for_file(mem, actual_scores, &norm)).unwrap_or_default();
        let parent = norm.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let mut siblings: Vec<FileMapSibling> = idx
            .files
            .iter()
            .filter(|f| {
                let f_parent = f.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                f_parent == parent && f.path != norm
            })
            .map(|f| {
                let public_symbols = f
                    .symbols
                    .iter()
                    .filter(|s| s.public && !s.test && s.kind != "var")
                    .cloned()
                    .collect();
                FileMapSibling {
                    path: f.path.clone(),
                    public_symbols,
                }
            })
            .collect();
        siblings.sort_by(|a, b| a.path.cmp(&b.path));
        siblings.truncate(5);

        FileMapResult {
            path: norm,
            kind: FileMapKind::NewFile,
            direct_rules,
            neighbor_rules: Vec::new(),
            imports: Vec::new(),
            imported_by: Vec::new(),
            symbols: Vec::new(),
            siblings,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use crate::learn::{self, Claim};
    use crate::score::{self, View};

    #[test]
    fn format_round_trip() {
        let index = ProjectIndex {
            files: vec![FileEntry {
                path: "src/lib.rs".to_string(),
                lang: "rust".to_string(),
                size: 100,
                mtime: 123456,
                hash: 0xDEADBEEF,
                    status: "-".to_string(),
                symbols: vec![Symbol {
                    name: "run".to_string(),
                    kind: "fn".to_string(),
                    line: 10,
                    end_line: 20,
                    public: true,
                    parent: None,
                    test: false,
                    signature: "(m: &Memory) -> bool".to_string(),
                    decorators: vec!["@inline".to_string()],
                }],
                imports: vec![Import {
                    raw: "crate::model::Node".to_string(),
                    resolved: Some("src/model.rs".to_string()),
                }],
                calls: vec![Call {
                    from: "run".to_string(),
                    name: "saturate".to_string(),
                    line: 15,
                }],
            }],
        };

        let formatted = format_index(&index);
        let parsed = parse_index(&formatted).expect("parsed successfully");
        assert_eq!(parsed, index);
    }

    #[test]
    fn reject_foreign_header_and_rebuild() {
        // v1 index is rejected so it rebuilds to v2
        let v1_tsv = "cthg-indice 1\nF\tsrc/lib.rs\trust\t100\t123456\tdeadbeef\n";
        assert!(parse_index(v1_tsv).is_err());
        let bad_tsv = "cthg-indice 99\nF\tsrc/lib.rs\trust\t100\t123456\tdeadbeef\n";
        assert!(parse_index(bad_tsv).is_err());
    }

    #[test]
    fn cross_language_callers_isolation() {
        let mut idx = ProjectIndex::default();
        idx.files.push(FileEntry {
            path: "app.py".to_string(),
            lang: "python".to_string(),
            size: 10,
            mtime: 1,
            hash: 1,
                    status: "-".to_string(),
            symbols: vec![Symbol {
                name: "save".to_string(),
                kind: "fn".to_string(),
                line: 1,
                end_line: 2,
                public: true,
                parent: None,
                test: false,
                signature: "() -> None".to_string(),
                decorators: vec![],
            }],
            imports: vec![],
            calls: vec![],
        });
        idx.files.push(FileEntry {
            path: "frontend.js".to_string(),
            lang: "js".to_string(),
            size: 10,
            mtime: 1,
            hash: 2,
                    status: "-".to_string(),
            symbols: vec![],
            imports: vec![],
            calls: vec![Call {
                from: "handleClick".to_string(),
                name: "save".to_string(),
                line: 5,
            }],
        });
        idx.files.push(FileEntry {
            path: "index.html".to_string(),
            lang: "html".to_string(),
            size: 10,
            mtime: 1,
            hash: 3,
                    status: "-".to_string(),
            symbols: vec![],
            imports: vec![],
            calls: vec![Call {
                from: "button#btn".to_string(),
                name: "save".to_string(),
                line: 10,
            }],
        });

        // Python save has NO callers from JS or HTML because they are in different language families
        let res = callers(&idx, "save");
        assert!(res.callers.is_empty(), "Python save should not be called by JS/HTML");
    }

    #[test]
    fn incremental_update_skips_unchanged_and_drops_deleted() {
        let disk_files = RefCell::new(HashMap::<String, (u64, u64, Vec<u8>)>::new());
        disk_files.borrow_mut().insert(
            "src/a.rs".to_string(),
            (20, 1000, b"pub fn a() { b(); }\n".to_vec()),
        );
        disk_files.borrow_mut().insert(
            "src/b.rs".to_string(),
            (15, 1000, b"pub fn b() {}\n".to_vec()),
        );

        let stored_index = RefCell::new(None::<String>);

        let probe = IndexProbe {
            list_files: &|| disk_files.borrow().keys().cloned().collect(),
            file_meta: &|p| disk_files.borrow().get(p).map(|(s, m, _)| (*s, *m)),
            read_file_bytes: &|p| disk_files.borrow().get(p).map(|(_, _, b)| b.clone()),
            read_index: &|| stored_index.borrow().clone(),
            write_index: &|_s| Ok(()),
        };

        // First run: 2 scanned
        let (idx1, stats1) = update_index(&probe, false).unwrap();
        assert_eq!(stats1.rescanned, 2);
        assert_eq!(stats1.unchanged, 0);
        *stored_index.borrow_mut() = Some(format_index(&idx1));

        // Second run: 0 rescanned, 2 unchanged
        let (_idx2, stats2) = update_index(&probe, false).unwrap();
        assert_eq!(stats2.rescanned, 0);
        assert_eq!(stats2.unchanged, 2);

        // Delete src/b.rs
        disk_files.borrow_mut().remove("src/b.rs");
        let (idx3, stats3) = update_index(&probe, false).unwrap();
        assert_eq!(stats3.removed, 1);
        assert_eq!(idx3.files.len(), 1);
    }

    #[test]
    fn callers_warns_ambiguity_when_more_than_3_files_define_symbol() {
        let mut index = ProjectIndex::default();
        for i in 1..=4 {
            index.files.push(FileEntry {
                path: format!("src/f{i}.rs"),
                lang: "rust".to_string(),
                size: 10,
                mtime: 10,
                hash: i as u32,
                    status: "-".to_string(),
                symbols: vec![Symbol {
                    name: "common".to_string(),
                    kind: "fn".to_string(),
                    line: 1,
                    end_line: 5,
                    public: true,
                    parent: None,
                    test: false,
                    signature: String::new(),
                    decorators: Vec::new(),
                }],
                imports: Vec::new(),
                calls: Vec::new(),
            });
        }
        // File 5 calls common
        index.files.push(FileEntry {
            path: "src/f5.rs".to_string(),
            lang: "rust".to_string(),
            size: 10,
            mtime: 10,
            hash: 5,
                    status: "-".to_string(),
            symbols: Vec::new(),
            imports: vec![Import {
                raw: "use crate::f1::common;".to_string(),
                resolved: Some("src/f1.rs".to_string()),
            }],
            calls: vec![Call {
                from: "caller".to_string(),
                name: "common".to_string(),
                line: 3,
            }],
        });

        let res = callers(&index, "common");
        assert!(res.is_ambiguous);
        assert_eq!(res.def_files_count, 4);
        assert_eq!(res.callers.len(), 1);
        assert!(res.callers[0].is_prioritized);
    }

    fn test_node(m: &mut Memory, path: &str, flags: u8) -> u32 {
        m.add_node(Node {
            id: 0,
            kind: NodeKind::Rule,
            origin: Origin::User,
            flags,
            path: path.to_string(),
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
            label: String::new(),
            text: String::new(),
            aliases: Vec::new(),
        })
    }

    #[test]
    fn map_includes_neighbor_rules_and_excludes_proposals() {
        let mut m = Memory::default();
        // #1 anchored to src/a.rs
        let n1 = test_node(&mut m, "src/a.rs", 0);
        // #2 anchored to src/b.rs
        let n2 = test_node(&mut m, "src/b.rs", 0);
        // #3 proposal anchored to src/b.rs
        let _n3 = test_node(&mut m, "src/b.rs", flags::PROPOSED);

        let scores = vec![
            Score { visible: true, chi: 0.9, ..Default::default() },
            Score { visible: true, chi: 0.9, ..Default::default() },
            Score { visible: true, chi: 0.5, ..Default::default() },
        ];

        let direct = anchored_rules_for_file(&m, &scores, "src/a.rs");
        assert_eq!(direct, vec![n1]);

        let neighbors = vec!["src/b.rs".to_string()];
        let neighbor_rules = neighbor_rules_for_file(&m, &scores, &neighbors, &direct);
        assert_eq!(neighbor_rules, vec![n2], "excludes proposals and direct rules");
    }

    #[test]
    fn fallback_outside_git_with_closure_probe() {
        let fake_dir_files = vec![
            "src/main.rs".to_string(),
            "target/debug/build.rs".to_string(),
            "node_modules/pkg/index.js".to_string(),
            ".cthg/something.rs".to_string(),
        ];

        let probe = IndexProbe {
            list_files: &|| fake_dir_files.clone(),
            file_meta: &|_p| Some((100, 100)),
            read_file_bytes: &|_| Some(b"fn main() {}\n".to_vec()),
            read_index: &|| None,
            write_index: &|_| Ok(()),
        };

        let (idx, _stats) = update_index(&probe, false).unwrap();
        assert_eq!(idx.files.len(), 1);
        assert_eq!(idx.files[0].path, "src/main.rs");
    }

    #[test]
    fn resolve_multiple_use_lines_real_hook_header() {
        let hook_content = b"
use super::learn;
use super::model::*;
use super::retrieve::{self, Query};
use super::score::{self, certainty_tag, Score, View};
use super::spatial::{self, Target};
use super::text;
use std::fs;
use std::path::{Path, PathBuf};

pub fn saturate() {}
";
        let disk_files = RefCell::new(HashMap::<String, (u64, u64, Vec<u8>)>::new());
        disk_files.borrow_mut().insert(
            "crates/cthg-core/src/hook.rs".to_string(),
            (hook_content.len() as u64, 1000, hook_content.to_vec()),
        );
        for sibling in &["learn", "model", "retrieve", "score", "spatial", "text"] {
            disk_files.borrow_mut().insert(
                format!("crates/cthg-core/src/{sibling}.rs"),
                (10, 1000, b"pub fn dummy() {}\n".to_vec()),
            );
        }

        let probe = IndexProbe {
            list_files: &|| disk_files.borrow().keys().cloned().collect(),
            file_meta: &|p| disk_files.borrow().get(p).map(|(s, m, _)| (*s, *m)),
            read_file_bytes: &|p| disk_files.borrow().get(p).map(|(_, _, b)| b.clone()),
            read_index: &|| None,
            write_index: &|_| Ok(()),
        };

        let (idx, _stats) = update_index(&probe, false).unwrap();
        let (imports, _) = file_neighbors(&idx, "crates/cthg-core/src/hook.rs");

        let expected = vec![
            "crates/cthg-core/src/learn.rs".to_string(),
            "crates/cthg-core/src/model.rs".to_string(),
            "crates/cthg-core/src/retrieve.rs".to_string(),
            "crates/cthg-core/src/score.rs".to_string(),
            "crates/cthg-core/src/spatial.rs".to_string(),
            "crates/cthg-core/src/text.rs".to_string(),
        ];
        assert_eq!(imports, expected);
    }

    #[test]
    fn resolve_imported_by_from_other_side() {
        let hook_content = b"pub fn saturate() {}\n";
        let eval_content = b"
use super::hook::{self, SessionState};
use super::import;
use super::model::*;
";
        let cargo_toml = b"[package]\nname = \"cthg-core\"\n";

        let disk_files = RefCell::new(HashMap::<String, (u64, u64, Vec<u8>)>::new());
        disk_files.borrow_mut().insert(
            "crates/cthg-core/Cargo.toml".to_string(),
            (cargo_toml.len() as u64, 1000, cargo_toml.to_vec()),
        );
        disk_files.borrow_mut().insert(
            "crates/cthg-core/src/hook.rs".to_string(),
            (hook_content.len() as u64, 1000, hook_content.to_vec()),
        );
        disk_files.borrow_mut().insert(
            "crates/cthg-core/src/eval.rs".to_string(),
            (eval_content.len() as u64, 1000, eval_content.to_vec()),
        );

        let probe = IndexProbe {
            list_files: &|| disk_files.borrow().keys().cloned().collect(),
            file_meta: &|p| disk_files.borrow().get(p).map(|(s, m, _)| (*s, *m)),
            read_file_bytes: &|p| disk_files.borrow().get(p).map(|(_, _, b)| b.clone()),
            read_index: &|| None,
            write_index: &|_| Ok(()),
        };

        let (idx, _stats) = update_index(&probe, false).unwrap();
        let (_, imported_by) = file_neighbors(&idx, "crates/cthg-core/src/hook.rs");

        assert_eq!(imported_by, vec!["crates/cthg-core/src/eval.rs".to_string()]);
    }
    #[test]
    fn test_scanner_panic_does_not_break_index() {
        let files = vec!["good.rs".to_string(), "panic.rs".to_string()];
        let list_fn = || files.clone();
        let meta_fn = |_p: &str| Some((50, 100));
        let read_fn = |p: &str| {
            if p == "panic.rs" {
                Some(b"PANIC_TRIGGER invalid code".to_vec())
            } else {
                Some(b"pub fn good_function() {}".to_vec())
            }
        };
        let index_buf = std::sync::Mutex::new(None);
        let read_idx = || index_buf.lock().unwrap().clone();
        let write_idx = |content: &str| {
            *index_buf.lock().unwrap() = Some(content.to_string());
            Ok(())
        };

        let probe = IndexProbe {
            list_files: &list_fn,
            file_meta: &meta_fn,
            read_file_bytes: &read_fn,
            read_index: &read_idx,
            write_index: &write_idx,
        };

        let res = update_index(&probe, true);
        assert!(res.is_ok());
        let (idx, stats) = res.unwrap();
        assert_eq!(stats.failed_files, vec!["panic.rs".to_string()]);
        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.total_symbols, 1);

        let good = idx.get_file("good.rs").unwrap();
        assert_eq!(good.status, "-");
        assert_eq!(good.symbols.len(), 1);
        assert_eq!(good.symbols[0].name, "good_function");

        let bad = idx.get_file("panic.rs").unwrap();
        assert_eq!(bad.status, "error");
        assert_eq!(bad.symbols.len(), 0);

        // Format and parse preservation
        let formatted = format_index(&idx);
        assert!(formatted.contains("F\tpanic.rs\trust\t50\t100\t"));
        assert!(formatted.contains("\terror\n"));
        let parsed = parse_index(&formatted).unwrap();
        assert_eq!(parsed.get_file("panic.rs").unwrap().status, "error");
    }

    #[test]
    fn test_file_map_three_cases() {
        let mut idx = ProjectIndex::default();
        idx.files.push(FileEntry {
            path: "src/main.rs".to_string(),
            lang: "rust".to_string(),
            size: 100,
            mtime: 1,
            hash: 1,
            status: "-".to_string(),
            symbols: vec![Symbol {
                name: "main".to_string(),
                kind: "fn".to_string(),
                line: 1,
                end_line: 10,
                public: true,
                parent: None,
                test: false,
                signature: "()".to_string(),
                decorators: vec![],
            }],
            imports: vec![crate::lang_rust::Import {
                raw: "use crate::cli;".to_string(),
                resolved: Some("src/cli.rs".to_string()),
            }],
            calls: vec![],
        });
        idx.files.push(FileEntry {
            path: "src/cli.rs".to_string(),
            lang: "rust".to_string(),
            size: 50,
            mtime: 1,
            hash: 2,
            status: "-".to_string(),
            symbols: vec![Symbol {
                name: "parse".to_string(),
                kind: "fn".to_string(),
                line: 5,
                end_line: 15,
                public: true,
                parent: None,
                test: false,
                signature: "() -> Args".to_string(),
                decorators: vec![],
            }],
            imports: vec![],
            calls: vec![],
        });

        let mut m = Memory::default();
        let main_claim = Claim::rule("Regla de main.rs", "src/main.rs");
        let cli_claim = Claim::rule("Regla de cli.rs", "src/cli.rs");
        let src_claim = Claim::rule("Regla de src general", "src");
        let cargo_claim = Claim::rule("Regla de Cargo.toml sobre tempfile", "Cargo.toml");

        learn::learn(&mut m, &main_claim, 10.0);
        learn::learn(&mut m, &cli_claim, 11.0);
        learn::learn(&mut m, &src_claim, 12.0);
        learn::learn(&mut m, &cargo_claim, 13.0);

        let scores = score::score_all(&m, &View::all());

        // Case 1: está en el índice -> Indexed
        let map_main = file_map(&idx, Some(&m), &scores, "src/main.rs", true);
        assert_eq!(map_main.kind, FileMapKind::Indexed);
        // Direct rules should include main.rs and src (folder ancestor)
        assert_eq!(map_main.direct_rules.len(), 2);
        // Neighbor rules should include cli.rs (imported by main.rs)
        assert_eq!(map_main.neighbor_rules.len(), 1);
        assert_eq!(map_main.imports, vec!["src/cli.rs".to_string()]);
        assert_eq!(map_main.symbols.len(), 1);
        assert!(map_main.siblings.is_empty());

        // Case 2: existe en disco pero no está en el índice -> NotIndexed
        let map_cargo = file_map(&idx, Some(&m), &scores, "Cargo.toml", true);
        assert_eq!(map_cargo.kind, FileMapKind::NotIndexed);
        assert_eq!(map_cargo.direct_rules.len(), 1); // Cargo.toml rule
        assert!(map_cargo.neighbor_rules.is_empty());
        assert!(map_cargo.imports.is_empty());
        assert!(map_cargo.symbols.is_empty());
        assert!(map_cargo.siblings.is_empty());

        // Case 3: no existe en disco -> NewFile
        let map_new = file_map(&idx, Some(&m), &scores, "src/nuevo.rs", false);
        assert_eq!(map_new.kind, FileMapKind::NewFile);
        // Direct rules should include src rule
        assert_eq!(map_new.direct_rules.len(), 1);
        assert!(map_new.neighbor_rules.is_empty());
        // Siblings in src/ should be cli.rs and main.rs (sorted, max 5)
        assert_eq!(map_new.siblings.len(), 2);
        assert_eq!(map_new.siblings[0].path, "src/cli.rs");
        assert_eq!(map_new.siblings[0].public_symbols[0].name, "parse");
        assert_eq!(map_new.siblings[1].path, "src/main.rs");
        assert_eq!(map_new.siblings[1].public_symbols[0].name, "main");
    }

}
