//! Codebase analyzer.
//!
//! Walks a set of source files and produces a [`Model`] that serialises to the
//! exact JSON shape the Codebase Visualizer front-end consumes:
//! `nodes`, `edges`, `detail`, `folders`, `sub`, `fnx`, `fieldx`, plus a
//! repository name and a plain-English architecture summary.
//!
//! The extraction is heuristic and line/brace based (no heavy parser
//! dependency) so it works across Rust, Python, JavaScript/TypeScript, Go,
//! Java and C/C++.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

mod lang;
use lang::Lang;

/// A source file handed to the analyzer (path is repo-relative).
#[derive(Debug, Clone)]
pub struct InputFile {
    pub path: String,
    pub text: String,
}

// ── Serialised output model (matches the front-end data contract) ──────────

#[derive(Debug, Serialize)]
pub struct Model {
    pub version: &'static str,
    #[serde(rename = "repoName")]
    pub repo_name: String,
    pub language: String,
    pub arch: String,
    pub nodes: Vec<NodeOut>,
    pub edges: Vec<[String; 2]>,
    pub detail: BTreeMap<String, DetailOut>,
    pub folders: Vec<FolderOut>,
    pub sub: BTreeMap<String, SubOut>,
    pub fnx: Vec<[String; 4]>,
    pub fieldx: Vec<[String; 4]>,
}

#[derive(Debug, Serialize)]
pub struct NodeOut {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub loc: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    pub orphan: bool,
}

#[derive(Debug, Serialize)]
pub struct DetailOut {
    pub path: String,
    pub summary: String,
    pub code: Vec<(String, String)>,
    pub loc: usize,
    pub tests: Vec<String>,
    pub callers: Vec<String>,
    pub callees: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub badges: Vec<(String, String)>,
}

#[derive(Debug, Serialize)]
pub struct FolderOut {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct SubOut {
    pub fns: Vec<FnOut>,
    pub structs: Vec<StructOut>,
}

#[derive(Debug, Serialize)]
pub struct FnOut {
    pub id: String,
    pub label: String,
    pub sig: String,
    pub calls: Vec<String>,
    pub code: Vec<(String, String)>,
    pub loc: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub doc: String,
}

#[derive(Debug, Serialize)]
pub struct StructOut {
    pub id: String,
    pub label: String,
    pub sig: String,
    pub fields: Vec<String>,
    pub code: Vec<(String, String)>,
    pub loc: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub doc: String,
}

// ── Internal working types ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct FuncSym {
    id: String,
    name: String,
    line: usize,
    sig: String,
    doc: String,
    body: String,
    is_test: bool,
    is_pub: bool,
}

#[derive(Debug, Clone)]
struct TypeSym {
    id: String,
    name: String,
    line: usize,
    sig: String,
    doc: String,
    body: String,
}

#[derive(Debug)]
struct FileInfo {
    id: String,
    label: String,
    path: String,
    dir: String,
    lang: Lang,
    text: String,
    loc: usize,
    funcs: Vec<FuncSym>,
    types: Vec<TypeSym>,
    file_doc: String,
    kind: String,
    has_io: bool,
    has_unsafe: bool,
    /// lowercase identifier set appearing in this file (for mention edges)
    idents: HashSet<String>,
}

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "dist",
    "build",
    "__pycache__",
    ".next",
    ".nuxt",
    "vendor",
    ".idea",
    ".vscode",
];

/// Walk a directory tree on disk and collect readable source files.
pub fn scan_dir(root: &Path, max_file_bytes: u64) -> io::Result<(String, Vec<InputFile>)> {
    let repo_name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .to_string();
    let mut files = Vec::new();
    collect(root, root, max_file_bytes, &mut files)?;
    Ok((repo_name, files))
}

/// Recursively walk `dir`, skipping ignored/hidden folders and oversized files, collecting readable source files as repo-relative `InputFile`s.
fn collect(root: &Path, dir: &Path, max: u64, out: &mut Vec<InputFile>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if IGNORED_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            collect(root, &path, max, out)?;
        } else if Lang::from_path(&path).is_some() {
            if let Ok(meta) = entry.metadata() {
                if meta.len() > max {
                    continue;
                }
            }
            if let Ok(text) = fs::read_to_string(&path) {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(InputFile { path: rel, text });
            }
        }
    }
    Ok(())
}

/// Analyze a set of files into the front-end model.
pub fn analyze(repo_name: &str, mut files: Vec<InputFile>) -> Model {
    files.retain(|f| Lang::from_ext(&f.path).is_some() && !f.text.is_empty());
    files.sort_by(|a, b| a.path.cmp(&b.path));

    // First pass: per-file symbol extraction.
    let mut used_ids: HashSet<String> = HashSet::new();
    let mut infos: Vec<FileInfo> = Vec::new();
    for f in &files {
        let lang = Lang::from_ext(&f.path).unwrap();
        let id = unique_id(&f.path, &mut used_ids);
        let label = display_label(&f.path);
        let dir = dir_of(&f.path);
        let loc = f.text.lines().count();
        let (funcs, types) = lang.extract(&f.text);
        let file_doc = lang.file_doc(&f.text);
        let has_io = lang.has_io(&f.text);
        let has_unsafe = f.text.contains("unsafe ") || f.text.contains("unsafe{");
        let mut idents = HashSet::new();
        for w in lang::idents(&f.text) {
            idents.insert(w.to_lowercase());
        }
        infos.push(FileInfo {
            id,
            label,
            path: f.path.clone(),
            dir,
            lang,
            text: f.text.clone(),
            loc,
            funcs,
            types,
            file_doc,
            kind: String::new(),
            has_io,
            has_unsafe,
            idents,
        });
    }

    // Classify each file's dominant role.
    for info in &mut infos {
        info.kind = classify(info);
    }

    // Ownership maps: symbol name -> owning file id (first definer wins).
    let mut type_owner: HashMap<String, String> = HashMap::new();
    let mut fn_owner: HashMap<String, String> = HashMap::new();
    let mut name_to_file: HashMap<String, String> = HashMap::new();
    for info in &infos {
        name_to_file
            .entry(file_stem(&info.path))
            .or_insert_with(|| info.id.clone());
        for t in &info.types {
            type_owner.entry(t.name.clone()).or_insert(info.id.clone());
        }
        for f in &info.funcs {
            fn_owner.entry(f.name.clone()).or_insert(info.id.clone());
        }
    }

    // ── File-level edges (dependencies) ──
    let mut edge_set: BTreeSet<(String, String)> = BTreeSet::new();

    // (a) import / use based edges.
    for info in &infos {
        for module in info.lang.imports(&info.text) {
            if let Some(target) = resolve_module(&module, info, &infos, &name_to_file) {
                if target != info.id {
                    edge_set.insert((info.id.clone(), target));
                }
            }
        }
    }

    // (b) symbol-mention edges: this file references a symbol owned elsewhere.
    let id_to_idx: HashMap<String, usize> =
        infos.iter().enumerate().map(|(i, f)| (f.id.clone(), i)).collect();
    for info in &infos {
        let mut targets: BTreeSet<String> = BTreeSet::new();
        for (name, owner) in type_owner.iter().chain(fn_owner.iter()) {
            if owner == &info.id {
                continue;
            }
            if name.len() < 4 || lang::is_common_word(name) {
                continue;
            }
            if info.idents.contains(&name.to_lowercase()) {
                targets.insert(owner.clone());
            }
        }
        for t in targets {
            edge_set.insert((info.id.clone(), t));
        }
    }

    let edges: Vec<[String; 2]> = edge_set
        .into_iter()
        .map(|(a, b)| [a, b])
        .collect();

    // Indegree / fan-in.
    let mut indeg: HashMap<String, usize> = infos.iter().map(|f| (f.id.clone(), 0)).collect();
    let mut callers_of: HashMap<String, Vec<String>> = HashMap::new();
    let mut callees_of: HashMap<String, Vec<String>> = HashMap::new();
    for e in &edges {
        *indeg.entry(e[1].clone()).or_insert(0) += 1;
        callers_of.entry(e[1].clone()).or_default().push(e[0].clone());
        callees_of.entry(e[0].clone()).or_default().push(e[1].clone());
    }
    let max_fanin = indeg.values().copied().max().unwrap_or(0);
    let avg_loc = if infos.is_empty() {
        0.0
    } else {
        infos.iter().map(|f| f.loc).sum::<usize>() as f64 / infos.len() as f64
    };

    // ── Layout (layered, left → right pipeline) ──
    let positions = layout(&infos, &edges, &indeg);

    // ── Cross-file edges for the bottom panel ──
    let mut fnx: BTreeSet<[String; 4]> = BTreeSet::new();
    let mut fieldx: BTreeSet<[String; 4]> = BTreeSet::new();
    for info in &infos {
        let local_fns: HashSet<&str> = info.funcs.iter().map(|f| f.name.as_str()).collect();
        for f in &info.funcs {
            for callee in info.lang.calls_in(&f.body) {
                if local_fns.contains(callee.as_str()) {
                    continue;
                }
                if let Some(owner) = fn_owner.get(&callee) {
                    if owner != &info.id {
                        if let Some(oidx) = id_to_idx.get(owner) {
                            if infos[*oidx].funcs.iter().any(|x| x.name == callee) {
                                fnx.insert([
                                    info.id.clone(),
                                    f.id.clone(),
                                    owner.clone(),
                                    callee.clone(),
                                ]);
                            }
                        }
                    }
                }
            }
        }
        let local_types: HashSet<&str> = info.types.iter().map(|t| t.name.as_str()).collect();
        for t in &info.types {
            for refd in lang::type_refs(&t.body) {
                if local_types.contains(refd.as_str()) {
                    continue;
                }
                if let Some(owner) = type_owner.get(&refd) {
                    if owner != &info.id {
                        fieldx.insert([
                            info.id.clone(),
                            t.id.clone(),
                            owner.clone(),
                            refd.clone(),
                        ]);
                    }
                }
            }
        }
    }

    // ── Build outputs ──
    let mut nodes = Vec::new();
    let mut detail = BTreeMap::new();
    let mut sub = BTreeMap::new();

    // Test descriptions: associate test fns project-wide to the files they exercise.
    let test_index = build_test_index(&infos);

    for info in &infos {
        let pos = positions.get(&info.id).copied().unwrap_or((60.0, 60.0));
        let orphan = indeg.get(&info.id).copied().unwrap_or(0) == 0 && info.kind != "entry";

        // risks + badges
        let mut risks: Vec<String> = Vec::new();
        let mut badges: Vec<(String, String)> = Vec::new();
        if info.loc > 300 {
            risks.push(format!(
                "{} lines — large module; consider splitting into smaller units.",
                info.loc
            ));
            badges.push(("LARGE".into(), "r".into()));
        } else if avg_loc > 0.0 && info.loc as f64 > avg_loc * 3.0 {
            risks.push(format!(
                "{} lines — about {}× the average module; a candidate for extraction.",
                info.loc,
                (info.loc as f64 / avg_loc).round() as usize
            ));
        }
        if max_fanin >= 3 && indeg.get(&info.id).copied().unwrap_or(0) == max_fanin {
            risks.push("Highest fan-in in the graph — changes here ripple widely.".into());
            badges.push(("high fan-in".into(), "n".into()));
        }
        if orphan {
            risks.push("No live callers reach this module — possible dead code.".into());
            badges.push(("ORPHAN".into(), "r".into()));
        }
        if info.has_unsafe {
            risks.push("Contains `unsafe` blocks — review memory-safety assumptions.".into());
        }
        if info.has_io {
            badges.push(("IO".into(), "n".into()));
        }
        if info.kind == "struct" {
            badges.push(("core type".into(), "n".into()));
        }

        let tests = test_index.get(&info.id).cloned().unwrap_or_default();
        if tests.is_empty() && info.loc > 60 {
            badges.push(("no tests".into(), "n".into()));
        }
        if !risks.is_empty() {
            // hotspot marker badge first
            badges.insert(0, ("HOTSPOT".into(), "r".into()));
        }
        // de-dup badges by label
        let mut seen = HashSet::new();
        badges.retain(|b| seen.insert(b.0.clone()));

        let summary = make_summary(info, &callees_of, &callers_of, orphan);
        let code = info.lang.snippet(info);

        let callers = sorted_labels(callers_of.get(&info.id), &infos);
        let callees = sorted_labels(callees_of.get(&info.id), &infos);

        nodes.push(NodeOut {
            id: info.id.clone(),
            label: info.label.clone(),
            kind: info.kind.clone(),
            x: pos.0,
            y: pos.1,
            loc: info.loc,
            diff: None,
            orphan,
        });

        detail.insert(
            info.id.clone(),
            DetailOut {
                path: info.path.clone(),
                summary,
                code,
                loc: info.loc,
                tests,
                callers,
                callees,
                risks,
                badges,
            },
        );

        let fns = info
            .funcs
            .iter()
            .map(|f| {
                let local: HashSet<&str> =
                    info.funcs.iter().map(|x| x.name.as_str()).collect();
                let mut calls: Vec<String> = Vec::new();
                let mut seen = HashSet::new();
                for c in info.lang.calls_in(&f.body) {
                    if c != f.name && local.contains(c.as_str()) && seen.insert(c.clone()) {
                        if let Some(t) = info.funcs.iter().find(|x| x.name == c) {
                            calls.push(t.id.clone());
                        }
                    }
                }
                FnOut {
                    id: f.id.clone(),
                    label: f.name.clone(),
                    sig: if f.sig.is_empty() { "()".into() } else { f.sig.clone() },
                    calls,
                    code: info.lang.snippet_at(&info.text, f.line, 4000),
                    loc: f.body.lines().count().max(1),
                    doc: f.doc.clone(),
                }
            })
            .collect();

        let structs = info
            .types
            .iter()
            .map(|t| {
                let local: HashSet<&str> =
                    info.types.iter().map(|x| x.name.as_str()).collect();
                let mut fields: Vec<String> = Vec::new();
                let mut seen = HashSet::new();
                for r in lang::type_refs(&t.body) {
                    if r != t.name && local.contains(r.as_str()) && seen.insert(r.clone()) {
                        if let Some(o) = info.types.iter().find(|x| x.name == r) {
                            fields.push(o.id.clone());
                        }
                    }
                }
                StructOut {
                    id: t.id.clone(),
                    label: t.name.clone(),
                    sig: t.sig.clone(),
                    fields,
                    code: info.lang.snippet_at(&info.text, t.line, 4000),
                    loc: t.body.lines().count().max(1),
                    doc: t.doc.clone(),
                }
            })
            .collect();

        sub.insert(info.id.clone(), SubOut { fns, structs });
    }

    // ── Folders ──
    let folders = build_folders(&infos);

    // ── Architecture summary ──
    let arch = make_arch(repo_name, &infos, &edges, &indeg);

    // Dominant language (by file count) for the project meta.
    let mut lang_counts: HashMap<&str, usize> = HashMap::new();
    for info in &infos {
        *lang_counts.entry(info.lang.name()).or_insert(0) += 1;
    }
    let language = lang_counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(n, _)| n.to_lowercase())
        .unwrap_or_default();

    Model {
        version: "1",
        repo_name: repo_name.to_string(),
        language,
        arch,
        nodes,
        edges,
        detail,
        folders,
        sub,
        fnx: fnx.into_iter().collect(),
        fieldx: fieldx.into_iter().collect(),
    }
}

// ── Classification ─────────────────────────────────────────────────────────

/// Assign a file's dominant role (entry/struct/fn/file) in priority order per the card-type spec: entry, then type-dominated, then single-function, else plain file.
fn classify(info: &FileInfo) -> String {
    if is_entry(info) {
        return "entry".into();
    }
    let nf = info.funcs.len();
    let nt = info.types.len();
    // type-dominated
    if nt >= 1 && (nf == 0 || (nt >= nf && nf <= 2)) {
        return "struct".into();
    }
    // single function / tiny helper
    if nt == 0 && nf >= 1 && nf <= 2 {
        return "fn".into();
    }
    "file".into()
}

/// Detect whether a file is a program entry point (line/symbol based, so it is
/// not fooled by the string `"fn main("` appearing inside a literal).
fn is_entry(info: &FileInfo) -> bool {
    let stem = file_stem(&info.path);
    let has_main = info.funcs.iter().any(|f| f.name == "main" || f.name == "Main");
    match info.lang {
        Lang::Rust | Lang::Go | Lang::C | Lang::Cpp => has_main,
        Lang::Java => has_main || info.text.contains("static void main"),
        Lang::Python => {
            has_main
                || info.text.contains("if __name__")
                || stem == "__main__"
        }
        Lang::JavaScript | Lang::TypeScript => {
            matches!(stem.as_str(), "index" | "main" | "app" | "server" | "cli")
        }
    }
}

// ── Summaries ────────────────────────────────────────────────────────────

/// Build the inspector's plain-English summary for a file: prefer a real doc comment (file or first public symbol), else synthesise one from role, symbol counts and dependency degree.
fn make_summary(
    info: &FileInfo,
    callees: &HashMap<String, Vec<String>>,
    callers: &HashMap<String, Vec<String>>,
    orphan: bool,
) -> String {
    if !info.file_doc.is_empty() {
        return clamp_sentence(&info.file_doc);
    }
    // doc of first public function/type
    if let Some(f) = info.funcs.iter().find(|f| f.is_pub && !f.doc.is_empty()) {
        return clamp_sentence(&f.doc);
    }
    if let Some(f) = info.funcs.iter().find(|f| !f.doc.is_empty()) {
        return clamp_sentence(&f.doc);
    }
    if let Some(t) = info.types.iter().find(|t| !t.doc.is_empty()) {
        return clamp_sentence(&t.doc);
    }
    // generated description
    let role = match info.kind.as_str() {
        "entry" => "the entry point",
        "struct" => "a data-structure module",
        "fn" => "a small helper module",
        _ => "a source module",
    };
    let mut parts = vec![format!(
        "{} — {} written in {}.",
        info.label,
        role,
        info.lang.name()
    )];
    let nf = info.funcs.len();
    let nt = info.types.len();
    if nf > 0 || nt > 0 {
        parts.push(format!(
            "Defines {} and {}.",
            count_phrase(nf, "function", "functions"),
            count_phrase(nt, "type", "types")
        ));
    }
    let out_deg = callees.get(&info.id).map(|v| v.len()).unwrap_or(0);
    let in_deg = callers.get(&info.id).map(|v| v.len()).unwrap_or(0);
    if orphan {
        parts.push("Nothing in the graph depends on it.".into());
    } else {
        parts.push(format!(
            "Depended on by {} and reaches out to {}.",
            count_phrase(in_deg, "module", "modules"),
            count_phrase(out_deg, "module", "modules")
        ));
    }
    parts.join(" ")
}

/// Pluralise a count into a phrase, e.g. 0 -> "no modules", 1 -> "1 module", 3 -> "3 modules".
fn count_phrase(n: usize, one: &str, many: &str) -> String {
    match n {
        0 => format!("no {}", many),
        1 => format!("1 {}", one),
        _ => format!("{} {}", n, many),
    }
}

/// Trim an over-long doc comment to ~300 chars, breaking on a sentence boundary and appending an ellipsis when truncated.
fn clamp_sentence(s: &str) -> String {
    let s = s.trim();
    if s.len() <= 320 {
        return s.to_string();
    }
    let mut out = String::new();
    for ch in s.chars() {
        out.push(ch);
        if out.len() >= 300 && (ch == '.' || ch == '!' || ch == '?') {
            break;
        }
    }
    if out.len() < s.len() && !out.ends_with('.') {
        out.push('…');
    }
    out
}

/// Compose the architecture sticky-note paragraph: module/folder counts, languages, the entry point and most-depended-on hub, and the inferred link count.
fn make_arch(
    repo: &str,
    infos: &[FileInfo],
    edges: &[[String; 2]],
    indeg: &HashMap<String, usize>,
) -> String {
    if infos.is_empty() {
        return String::new();
    }
    let mut langs: BTreeSet<&str> = BTreeSet::new();
    for i in infos {
        langs.insert(i.lang.name());
    }
    let mut dirs: BTreeSet<&str> = BTreeSet::new();
    for i in infos {
        dirs.insert(if i.dir.is_empty() { "." } else { i.dir.as_str() });
    }
    let entry = infos
        .iter()
        .find(|i| i.kind == "entry")
        .or_else(|| infos.iter().min_by_key(|i| indeg.get(&i.id).copied().unwrap_or(0)));
    let langs_list = langs.into_iter().collect::<Vec<_>>().join(", ");
    let mut s = format!(
        "{}: {} across {}. Languages: {}.",
        repo,
        count_phrase(infos.len(), "module", "modules"),
        count_phrase(dirs.len(), "folder", "folders"),
        langs_list
    );
    if let Some(e) = entry {
        s.push_str(&format!(" Execution flows from {}", e.label));
        // most-depended-on module
        if let Some(hub) = infos
            .iter()
            .max_by_key(|i| indeg.get(&i.id).copied().unwrap_or(0))
        {
            if indeg.get(&hub.id).copied().unwrap_or(0) > 0 && hub.id != e.id {
                s.push_str(&format!(" toward the most-depended-on module {}", hub.label));
            }
        }
        s.push('.');
    }
    s.push_str(&format!(" {} dependency links inferred.", edges.len()));
    s
}

// ── Tests ──────────────────────────────────────────────────────────────────

/// Map each file id to humanised descriptions of the test functions that reference its symbols (the inspector's "Tests as docs").
fn build_test_index(infos: &[FileInfo]) -> HashMap<String, Vec<String>> {
    // Map symbol name -> owning file id.
    let mut owner: HashMap<String, String> = HashMap::new();
    for info in infos {
        for f in &info.funcs {
            owner.entry(f.name.clone()).or_insert(info.id.clone());
        }
        for t in &info.types {
            owner.entry(t.name.clone()).or_insert(info.id.clone());
        }
    }
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for info in infos {
        for f in &info.funcs {
            if !f.is_test {
                continue;
            }
            let desc = humanize(&f.name);
            // attach the test to files whose symbols it references
            let mut attached: BTreeSet<String> = BTreeSet::new();
            for (name, oid) in &owner {
                if name.len() >= 4
                    && !lang::is_common_word(name)
                    && f.body.contains(name.as_str())
                {
                    attached.insert(oid.clone());
                }
            }
            if attached.is_empty() {
                attached.insert(info.id.clone());
            }
            for fid in attached {
                let v = out.entry(fid).or_default();
                if v.len() < 5 && !v.contains(&desc) {
                    v.push(desc.clone());
                }
            }
        }
    }
    out
}

/// Turn a test function name like `test_parses_args` into a readable description ("parses args").
fn humanize(name: &str) -> String {
    let n = name
        .trim_start_matches("test_")
        .trim_start_matches("test");
    let n = n.trim_start_matches('_');
    let spaced = n.replace('_', " ");
    let spaced = spaced.trim();
    if spaced.is_empty() {
        name.to_string()
    } else {
        spaced.to_string()
    }
}

// ── Folders ──────────────────────────────────────────────────────────────

/// Group file ids by their parent directory into the left-panel folder tree (root-level files go under ".").
fn build_folders(infos: &[FileInfo]) -> Vec<FolderOut> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for info in infos {
        let dir = if info.dir.is_empty() {
            ".".to_string()
        } else {
            info.dir.clone()
        };
        map.entry(dir).or_default().push(info.id.clone());
    }
    map.into_iter()
        .map(|(name, files)| FolderOut {
            name,
            files: Some(files),
            vendor: None,
        })
        .collect()
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// Place file nodes on the 1360x600 virtual canvas as a left->right layered pipeline: column = cycle-safe BFS depth from the entry/root nodes, rows centred per column.
fn layout(
    infos: &[FileInfo],
    edges: &[[String; 2]],
    indeg: &HashMap<String, usize>,
) -> HashMap<String, (f64, f64)> {
    let ids: Vec<&str> = infos.iter().map(|i| i.id.as_str()).collect();
    // adjacency for BFS
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in edges {
        adj.entry(e[0].as_str()).or_default().push(e[1].as_str());
    }
    // roots = indegree 0 (or entry); fall back to first node
    let mut depth: HashMap<String, i32> = HashMap::new();
    let mut queue: std::collections::VecDeque<&str> = std::collections::VecDeque::new();
    for i in infos {
        if indeg.get(&i.id).copied().unwrap_or(0) == 0 || i.kind == "entry" {
            depth.insert(i.id.clone(), 0);
            queue.push_back(i.id.as_str());
        }
    }
    if depth.is_empty() {
        if let Some(first) = ids.first() {
            depth.insert(first.to_string(), 0);
            queue.push_back(first);
        }
    }
    // multi-source BFS (shortest path) — stable even when the graph has cycles
    while let Some(cur) = queue.pop_front() {
        let d = depth.get(cur).copied().unwrap_or(0);
        if let Some(next) = adj.get(cur) {
            for &nx in next {
                if !depth.contains_key(nx) {
                    depth.insert(nx.to_string(), d + 1);
                    queue.push_back(nx);
                }
            }
        }
    }
    for i in infos {
        depth.entry(i.id.clone()).or_insert(0);
    }
    let max_depth = depth.values().copied().max().unwrap_or(0).max(0);

    // group by column
    let mut cols: BTreeMap<i32, Vec<&str>> = BTreeMap::new();
    for i in infos {
        let d = depth.get(&i.id).copied().unwrap_or(0);
        cols.entry(d).or_default().push(i.id.as_str());
    }

    const CANVAS_W: f64 = 1360.0;
    const CANVAS_H: f64 = 600.0;
    const NW: f64 = 150.0;
    const NH: f64 = 84.0;
    let col_gap = if max_depth > 0 {
        ((CANVAS_W - NW - 80.0) / max_depth as f64).min(210.0)
    } else {
        0.0
    };
    let row_gap = 132.0;

    let mut pos = HashMap::new();
    for (d, members) in &cols {
        let x = 50.0 + (*d as f64) * col_gap;
        let n = members.len();
        let total_h = (n.saturating_sub(1)) as f64 * row_gap + NH;
        let start_y = if total_h < CANVAS_H {
            ((CANVAS_H - total_h) / 2.0).max(20.0)
        } else {
            20.0
        };
        for (i, id) in members.iter().enumerate() {
            let y = start_y + i as f64 * row_gap;
            pos.insert(id.to_string(), (round1(x), round1(y)));
        }
    }
    pos
}

/// Round to one decimal place to keep emitted coordinates compact.
fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// De-duplicate a list of node ids while preserving order (used for callers/callees).
fn sorted_labels(ids: Option<&Vec<String>>, _infos: &[FileInfo]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    if let Some(v) = ids {
        for id in v {
            if seen.insert(id.clone()) {
                out.push(id.clone());
            }
        }
    }
    out
}

/// Derive a stable, unique node id from a file path: the file stem, disambiguated with the parent folder for ambiguous names (mod/index/__init__) and a numeric suffix on collision.
fn unique_id(path: &str, used: &mut HashSet<String>) -> String {
    let stem = file_stem(path);
    let ambiguous = matches!(
        stem.as_str(),
        "mod" | "index" | "main" | "lib" | "__init__" | "__main__"
    );
    let mut base = if ambiguous {
        let parent = dir_of(path);
        let parent = parent.rsplit('/').next().unwrap_or("");
        if parent.is_empty() || stem == "main" || stem == "lib" {
            stem.clone()
        } else {
            format!("{}_{}", parent, stem)
        }
    } else {
        stem.clone()
    };
    base = sanitize(&base);
    if base.is_empty() {
        base = "mod".into();
    }
    let mut candidate = base.clone();
    let mut n = 1;
    while used.contains(&candidate) {
        n += 1;
        candidate = format!("{}{}", base, n);
    }
    used.insert(candidate.clone());
    candidate
}

/// Replace any non-alphanumeric/underscore character with `_` so ids are safe map keys.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// Return a path's filename without its extension.
fn file_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => name.to_string(),
    }
}

/// Human-friendly node label: the filename, prefixed with its parent folder for ambiguous names like `mod.rs`/`index.js`.
fn display_label(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = file_stem(path);
    if matches!(stem.as_str(), "mod" | "index" | "__init__" | "__main__") {
        // include parent dir for clarity
        let dir = dir_of(path);
        if let Some(parent) = dir.rsplit('/').next() {
            if !parent.is_empty() {
                return format!("{}/{}", parent, name);
            }
        }
    }
    name.to_string()
}

/// Return the directory portion of a repo-relative path (empty for root-level files).
fn dir_of(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an `InputFile` from a path and source text (test helper).
    fn f(path: &str, text: &str) -> InputFile {
        InputFile { path: path.into(), text: text.into() }
    }

    /// Smoke test: a tiny Rust project classifies `main` as entry, `model` as a
    /// type module and `util` as a single-fn module, links `main` to both, flags
    /// the orphan, and exposes `model`'s types in the bottom-panel sub graph.
    #[test]
    fn classifies_and_links_a_small_rust_project() {
        let files = vec![
            f("src/main.rs", "//! Entry.\nuse crate::model;\nuse crate::util;\nfn main() { util::run(model::Cfg::new()); }"),
            f("src/util.rs", "//! Helper.\nuse crate::model;\npub fn run(c: model::Cfg) {}"),
            f("src/model.rs", "//! Types.\npub struct Cfg { pub n: u32 }\npub enum Mode { A, B }"),
            f("src/dead.rs", "fn unused() {}"),
        ];
        let m = analyze("demo", files);
        let kind = |id: &str| m.nodes.iter().find(|n| n.id == id).map(|n| n.kind.as_str());
        assert_eq!(kind("main"), Some("entry"));
        assert_eq!(kind("model"), Some("struct"));
        assert_eq!(kind("util"), Some("fn"));
        // main depends on model and util
        assert!(m.edges.contains(&["main".into(), "util".into()]));
        assert!(m.edges.contains(&["main".into(), "model".into()]));
        // dead.rs has no callers -> orphan
        assert!(m.nodes.iter().any(|n| n.id == "dead" && n.orphan));
        // model.rs exposes its types in the bottom-panel sub graph
        let model_sub = &m.sub["model"];
        assert!(model_sub.structs.iter().any(|s| s.label == "Cfg"));
    }
}

/// Resolve an import/use hint to the id of the file it refers to: handles relative JS/TS paths and picks the deepest path segment (e.g. `crate::cli::Cli` -> `cli`) that maps to a scanned file.
fn resolve_module(
    module: &str,
    from: &FileInfo,
    _infos: &[FileInfo],
    name_to_file: &HashMap<String, String>,
) -> Option<String> {
    // relative path resolution for JS/TS imports like ./foo or ../bar/baz
    if module.starts_with('.') {
        if let Some(base) = Path::new(&from.path).parent() {
            let joined = base.join(module.trim_start_matches("./"));
            let target = joined.to_string_lossy().replace('\\', "/");
            let tstem = file_stem(&target);
            if let Some(id) = name_to_file.get(&tstem) {
                return Some(id.clone());
            }
            // last segment of the relative path
            if let Some(seg) = module.rsplit(['/', '.']).find(|s| !s.is_empty()) {
                if let Some(id) = name_to_file.get(seg) {
                    return Some(id.clone());
                }
            }
        }
    }
    // Module/symbol path like `crate::cli::Cli` or `pkg.module.Symbol`:
    // pick the deepest segment that maps to a known file (the module name sits
    // just before the imported symbol, so prefer the last matching segment).
    let mut found: Option<String> = None;
    for seg in module.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if seg.is_empty() {
            continue;
        }
        if let Some(id) = name_to_file.get(seg) {
            if id != &from.id {
                found = Some(id.clone());
            }
        }
    }
    found
}
