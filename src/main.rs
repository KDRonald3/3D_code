use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct Args {
    input: PathBuf,
    output: PathBuf,
    max_file_bytes: u64,
}

#[derive(Debug, Clone)]
struct Node {
    id: String,
    label: String,
    kind: NodeKind,
    path: String,
    line: usize,
    language: String,
    doc: String,
    summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeKind {
    File,
    Function,
    Type,
}

#[derive(Debug, Clone)]
struct Edge {
    source: String,
    target: String,
    label: String,
}

#[derive(Debug)]
struct Graph {
    root: String,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    stats: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
struct PatternSet {
    function: SymbolPattern,
    data_type: SymbolPattern,
    doc_prefixes: &'static [&'static str],
}

#[derive(Debug, Clone)]
enum SymbolPattern {
    RustFunction,
    RustType,
    PythonFunction,
    PythonType,
    JsFunction,
    JsType,
    GoFunction,
    GoType,
    JavaLikeFunction,
    JavaLikeType,
}

#[derive(Debug)]
struct ScannedFile {
    node: Node,
    source: String,
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let root = args.input.canonicalize().map_err(|err| {
        format!(
            "failed to read input directory {}: {err}",
            args.input.display()
        )
    })?;

    let graph = scan_codebase(&root, args.max_file_bytes)?;
    let html = render_html(&graph);

    fs::write(&args.output, html)
        .map_err(|err| format!("failed to write {}: {err}", args.output.display()))?;

    println!(
        "Wrote {} with {} nodes and {} links.",
        args.output.display(),
        graph.nodes.len(),
        graph.edges.len()
    );
    println!(
        "Scanned {} files, found {} functions and {} data structures.",
        graph.stats.get("files").copied().unwrap_or_default(),
        graph.stats.get("functions").copied().unwrap_or_default(),
        graph.stats.get("types").copied().unwrap_or_default()
    );

    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut input = PathBuf::from(".");
    let mut output = PathBuf::from("codebase-map.html");
    let mut max_file_bytes = 500_000;
    let mut positional = Vec::new();
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "-o" | "--output" => {
                output = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--output requires a file path".to_string())?,
                );
            }
            "--max-file-bytes" => {
                max_file_bytes = args
                    .next()
                    .ok_or_else(|| "--max-file-bytes requires a number".to_string())?
                    .parse()
                    .map_err(|err| format!("invalid --max-file-bytes value: {err}"))?;
            }
            _ if arg.starts_with("--output=") => {
                output = PathBuf::from(arg.trim_start_matches("--output="));
            }
            _ if arg.starts_with("--max-file-bytes=") => {
                max_file_bytes = arg
                    .trim_start_matches("--max-file-bytes=")
                    .parse()
                    .map_err(|err| format!("invalid --max-file-bytes value: {err}"))?;
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}").into()),
            _ => positional.push(arg),
        }
    }

    if let Some(first) = positional.first() {
        input = PathBuf::from(first);
    }
    if positional.len() > 1 {
        return Err("too many positional arguments".into());
    }

    Ok(Args {
        input,
        output,
        max_file_bytes,
    })
}

fn print_help() {
    println!(
        "codebase-visualizer\n\nUSAGE:\n    codebase_visualizer [INPUT] [--output FILE] [--max-file-bytes BYTES]\n\nGenerates a self-contained searchable HTML graph of files, functions, data structures, docs, and simple references."
    );
}

fn scan_codebase(root: &Path, max_file_bytes: u64) -> Result<Graph> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut scanned_files = Vec::new();

    for path in collect_source_files(root, max_file_bytes)? {
        let source = fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let relative_path = relative_path(root, &path);
        let language = language_for(&path).unwrap_or("text").to_string();
        let file_id = node_id("file", &relative_path, 0, &relative_path);
        let file_node = Node {
            id: file_id.clone(),
            label: relative_path.clone(),
            kind: NodeKind::File,
            path: relative_path.clone(),
            line: 1,
            language: language.clone(),
            doc: String::new(),
            summary: summarize_file(&source),
        };

        scanned_files.push(ScannedFile {
            node: file_node.clone(),
            source: source.clone(),
        });
        nodes.push(file_node);

        if let Some(patterns) = patterns_for(&language) {
            for symbol in extract_symbols(&source, &relative_path, &language, patterns) {
                edges.push(Edge {
                    source: file_id.clone(),
                    target: symbol.id.clone(),
                    label: "contains".to_string(),
                });
                nodes.push(symbol);
            }
        }
    }

    edges.extend(reference_edges(&nodes, &scanned_files));

    let mut stats = BTreeMap::new();
    stats.insert(
        "files".to_string(),
        nodes
            .iter()
            .filter(|node| node.kind == NodeKind::File)
            .count(),
    );
    stats.insert(
        "functions".to_string(),
        nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Function)
            .count(),
    );
    stats.insert(
        "types".to_string(),
        nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Type)
            .count(),
    );

    Ok(Graph {
        root: root.display().to_string(),
        nodes,
        edges,
        stats,
    })
}

fn collect_source_files(root: &Path, max_file_bytes: u64) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_source_files_inner(root, max_file_bytes, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_source_files_inner(
    path: &Path,
    max_file_bytes: u64,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    if is_ignored_dir(path) {
        return Ok(());
    }

    for entry in
        fs::read_dir(path).map_err(|err| format!("failed to list {}: {err}", path.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_source_files_inner(&path, max_file_bytes, files)?;
        } else if path.is_file()
            && language_for(&path).is_some()
            && entry.metadata()?.len() <= max_file_bytes
        {
            files.push(path);
        }
    }

    Ok(())
}

fn is_ignored_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            matches!(
                name,
                ".git" | "target" | "node_modules" | ".venv" | "venv" | "dist" | "build"
            )
        })
        .unwrap_or(false)
}

fn language_for(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|ext| ext.to_str())? {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" | "jsx" => Some("javascript"),
        "ts" | "tsx" => Some("typescript"),
        "go" => Some("go"),
        "java" => Some("java"),
        "c" | "h" => Some("c"),
        "cc" | "cpp" | "hpp" => Some("cpp"),
        _ => None,
    }
}

fn patterns_for(language: &str) -> Option<PatternSet> {
    Some(match language {
        "rust" => PatternSet {
            function: SymbolPattern::RustFunction,
            data_type: SymbolPattern::RustType,
            doc_prefixes: &["///", "//!"],
        },
        "python" => PatternSet {
            function: SymbolPattern::PythonFunction,
            data_type: SymbolPattern::PythonType,
            doc_prefixes: &["#", "\"\"\"", "'''"],
        },
        "javascript" | "typescript" => PatternSet {
            function: SymbolPattern::JsFunction,
            data_type: SymbolPattern::JsType,
            doc_prefixes: &["//", "/*", "*"],
        },
        "go" => PatternSet {
            function: SymbolPattern::GoFunction,
            data_type: SymbolPattern::GoType,
            doc_prefixes: &["//"],
        },
        "java" | "c" | "cpp" => PatternSet {
            function: SymbolPattern::JavaLikeFunction,
            data_type: SymbolPattern::JavaLikeType,
            doc_prefixes: &["//", "/*", "*"],
        },
        _ => return None,
    })
}

fn extract_symbols(source: &str, path: &str, language: &str, patterns: PatternSet) -> Vec<Node> {
    let lines: Vec<&str> = source.lines().collect();
    let mut nodes = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        if let Some(name) = capture_symbol(&patterns.function, line) {
            nodes.push(symbol_node(
                NodeKind::Function,
                &name,
                path,
                index + 1,
                language,
                &lines,
                &patterns,
            ));
        }

        if let Some(name) = capture_symbol(&patterns.data_type, line) {
            nodes.push(symbol_node(
                NodeKind::Type,
                &name,
                path,
                index + 1,
                language,
                &lines,
                &patterns,
            ));
        }
    }

    nodes
}

fn capture_symbol(pattern: &SymbolPattern, line: &str) -> Option<String> {
    let trimmed = line.trim();
    let name = match pattern {
        SymbolPattern::RustFunction => {
            symbol_after_keywords(trimmed, &["pub", "async", "fn"], "fn")
        }
        SymbolPattern::RustType => {
            symbol_after_any_keyword(trimmed, &["struct", "enum", "trait", "type"])
        }
        SymbolPattern::PythonFunction => {
            let without_async = trimmed.strip_prefix("async ").unwrap_or(trimmed);
            without_async
                .strip_prefix("def ")
                .and_then(first_identifier)
        }
        SymbolPattern::PythonType => trimmed.strip_prefix("class ").and_then(first_identifier),
        SymbolPattern::JsFunction => capture_js_function(trimmed),
        SymbolPattern::JsType => symbol_after_any_keyword(trimmed, &["class", "interface", "type"]),
        SymbolPattern::GoFunction => capture_go_function(trimmed),
        SymbolPattern::GoType => trimmed.strip_prefix("type ").and_then(first_identifier),
        SymbolPattern::JavaLikeFunction => capture_java_like_function(trimmed),
        SymbolPattern::JavaLikeType => {
            symbol_after_any_keyword(trimmed, &["class", "struct", "enum", "interface"])
        }
    };

    name.filter(|name| !name.is_empty() && !is_keyword(name))
}

fn symbol_node(
    kind: NodeKind,
    name: &str,
    path: &str,
    line: usize,
    language: &str,
    lines: &[&str],
    patterns: &PatternSet,
) -> Node {
    let doc = preceding_doc(lines, line.saturating_sub(1), patterns.doc_prefixes);
    let summary = if doc.is_empty() {
        format!("{} discovered near line {}.", kind_label(&kind), line)
    } else {
        first_sentence(&doc)
    };

    Node {
        id: node_id(kind_label(&kind), path, line, name),
        label: name.to_string(),
        kind,
        path: path.to_string(),
        line,
        language: language.to_string(),
        doc,
        summary,
    }
}

fn preceding_doc(lines: &[&str], declaration_index: usize, prefixes: &[&str]) -> String {
    let mut docs = Vec::new();
    let mut index = declaration_index;
    while index > 0 {
        index -= 1;
        let trimmed = lines[index].trim();
        if trimmed.is_empty() {
            if docs.is_empty() {
                continue;
            }
            break;
        }

        if let Some(cleaned) = clean_doc_line(trimmed, prefixes) {
            if !cleaned.is_empty() {
                docs.push(cleaned);
            }
        } else {
            break;
        }
    }

    docs.reverse();
    docs.join(" ").trim().to_string()
}

fn clean_doc_line(line: &str, prefixes: &[&str]) -> Option<String> {
    for prefix in prefixes {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(rest.trim_matches('/').trim_matches('*').trim().to_string());
        }
    }
    None
}

fn summarize_file(source: &str) -> String {
    let non_empty = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    format!("{non_empty} non-empty lines scanned.")
}

fn reference_edges(nodes: &[Node], files: &[ScannedFile]) -> Vec<Edge> {
    let symbols: Vec<&Node> = nodes
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::Function | NodeKind::Type))
        .collect();
    let by_path: HashMap<&str, Vec<&Node>> =
        symbols.iter().fold(HashMap::new(), |mut map, node| {
            map.entry(node.path.as_str()).or_default().push(*node);
            map
        });

    let mut edges = Vec::new();
    let mut seen = HashSet::new();

    for file in files {
        let Some(local_symbols) = by_path.get(file.node.path.as_str()) else {
            continue;
        };

        for source_symbol in local_symbols {
            for target_symbol in &symbols {
                if source_symbol.id == target_symbol.id {
                    continue;
                }

                if source_mentions(&file.source, &target_symbol.label) {
                    let key = format!("{}->{}", source_symbol.id, target_symbol.id);
                    if seen.insert(key) {
                        edges.push(Edge {
                            source: source_symbol.id.clone(),
                            target: target_symbol.id.clone(),
                            label: "mentions".to_string(),
                        });
                    }
                }
            }
        }
    }

    edges
}

fn render_html(graph: &Graph) -> String {
    let graph_json = graph_to_json(graph);
    let escaped_title = escape_html(&format!("Codebase map · {}", graph.root));

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>{escaped_title}</title>
<style>
/* ── reset & tokens ──────────────────────────────────────────────────── */
*,*::before,*::after{{box-sizing:border-box;margin:0;padding:0}}
:root{{
  --bg:#07090f;
  --surface:#0d1117;
  --surface2:#131920;
  --border:rgba(255,255,255,.07);
  --border-hi:rgba(99,179,255,.45);
  --text:#e2e8f4;
  --text-muted:#7a8799;
  --accent-file:#3ddc84;
  --accent-fn:#4da6ff;
  --accent-type:#bf7bff;
  --accent-file-dim:rgba(61,220,132,.18);
  --accent-fn-dim:rgba(77,166,255,.18);
  --accent-type-dim:rgba(191,123,255,.18);
  --glow-file:0 0 18px 4px rgba(61,220,132,.35);
  --glow-fn:0 0 18px 4px rgba(77,166,255,.35);
  --glow-type:0 0 18px 4px rgba(191,123,255,.35);
  --radius:14px;
  --handle:5px;
  font-family:'Inter',ui-sans-serif,system-ui,sans-serif;
  color-scheme:dark;
}}
html,body{{height:100%;overflow:hidden;background:var(--bg);color:var(--text);font-size:13px;line-height:1.55}}

/* ── scrollbar styling ───────────────────────────────────────────────── */
::-webkit-scrollbar{{width:6px;height:6px}}
::-webkit-scrollbar-track{{background:transparent}}
::-webkit-scrollbar-thumb{{background:rgba(255,255,255,.12);border-radius:3px}}
::-webkit-scrollbar-thumb:hover{{background:rgba(255,255,255,.22)}}

/* ── layout ──────────────────────────────────────────────────────────── */
#app{{display:flex;flex-direction:row;height:100vh;overflow:hidden}}
#sidebar-left{{width:300px;min-width:160px;max-width:520px;display:flex;flex-direction:column;background:var(--surface);border-right:1px solid var(--border);flex-shrink:0;overflow:hidden}}
#sidebar-right{{width:320px;min-width:160px;max-width:520px;display:flex;flex-direction:column;background:var(--surface);border-left:1px solid var(--border);flex-shrink:0;overflow:hidden}}
#graph-area{{flex:1 1 0;min-width:0;position:relative;overflow:hidden;background:radial-gradient(ellipse 80% 60% at 50% 40%,#0c1628 0%,var(--bg) 100%)}}

/* ── resize handles ──────────────────────────────────────────────────── */
.handle{{width:var(--handle);cursor:col-resize;flex-shrink:0;background:var(--border);transition:background .15s;display:flex;align-items:center;justify-content:center;position:relative}}
.handle:hover,.handle.dragging{{background:var(--border-hi)}}
.handle::after{{content:'';display:block;width:2px;height:32px;border-radius:2px;background:rgba(255,255,255,.18)}}

/* ── sidebar inner ───────────────────────────────────────────────────── */
.sidebar-header{{padding:18px 16px 12px;border-bottom:1px solid var(--border);flex-shrink:0}}
.sidebar-body{{flex:1 1 0;overflow-y:auto;overflow-x:hidden;padding:14px 16px;display:flex;flex-direction:column;gap:10px}}
.logo{{display:flex;align-items:center;gap:8px;margin-bottom:6px}}
.logo-icon{{width:28px;height:28px;border-radius:8px;background:linear-gradient(135deg,#4da6ff 0%,#bf7bff 100%);display:flex;align-items:center;justify-content:center;font-size:15px;flex-shrink:0}}
.logo-text{{font-size:15px;font-weight:700;letter-spacing:-.3px;background:linear-gradient(90deg,#4da6ff,#bf7bff);-webkit-background-clip:text;-webkit-text-fill-color:transparent}}
.tagline{{color:var(--text-muted);font-size:11.5px;margin-top:2px}}

/* ── stats ───────────────────────────────────────────────────────────── */
.stats{{display:grid;grid-template-columns:repeat(3,1fr);gap:7px}}
.stat{{background:var(--surface2);border:1px solid var(--border);border-radius:10px;padding:9px 8px;text-align:center}}
.stat-value{{font-size:20px;font-weight:700;line-height:1}}
.stat-label{{font-size:10px;color:var(--text-muted);text-transform:uppercase;letter-spacing:.06em;margin-top:3px}}
.stat.file .stat-value{{color:var(--accent-file)}}
.stat.fn   .stat-value{{color:var(--accent-fn)}}
.stat.type .stat-value{{color:var(--accent-type)}}

/* ── search / filter ─────────────────────────────────────────────────── */
.search-wrap{{position:relative}}
.search-icon{{position:absolute;left:11px;top:50%;transform:translateY(-50%);opacity:.4;pointer-events:none;font-size:13px}}
input#search{{width:100%;padding:9px 12px 9px 32px;border-radius:10px;border:1px solid var(--border);background:var(--surface2);color:var(--text);font-size:13px;outline:none;transition:border-color .15s}}
input#search:focus{{border-color:var(--border-hi)}}
select#kind{{width:100%;padding:8px 10px;border-radius:10px;border:1px solid var(--border);background:var(--surface2);color:var(--text);font-size:12px;outline:none;cursor:pointer;appearance:none;background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='8' viewBox='0 0 12 8'%3E%3Cpath fill='%237a8799' d='M6 8 0 0h12z'/%3E%3C/svg%3E");background-repeat:no-repeat;background-position:right 10px center}}

/* ── result list ─────────────────────────────────────────────────────── */
.results-count{{font-size:11px;color:var(--text-muted);padding:2px 0 4px}}
.item-list{{display:flex;flex-direction:column;gap:5px}}
button.item{{text-align:left;border:1px solid var(--border);background:var(--surface2);color:var(--text);padding:9px 11px;border-radius:10px;cursor:pointer;transition:border-color .12s,background .12s,transform .1s;width:100%}}
button.item:hover{{border-color:rgba(99,179,255,.3);background:#0f1e30;transform:translateX(2px)}}
button.item.active{{border-color:var(--border-hi);background:rgba(77,166,255,.1)}}
button.item.active-file{{border-color:rgba(61,220,132,.5);background:rgba(61,220,132,.08)}}
button.item.active-type{{border-color:rgba(191,123,255,.5);background:rgba(191,123,255,.08)}}
.item-top{{display:flex;align-items:center;gap:6px;margin-bottom:3px}}
.item-name{{font-weight:600;font-size:13px}}
.item-path{{font-size:11px;color:var(--text-muted)}}
.item-summary{{font-size:11.5px;color:var(--text-muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;margin-top:2px}}

/* ── pill badges ─────────────────────────────────────────────────────── */
.pill{{display:inline-block;font-size:10px;font-weight:600;text-transform:uppercase;letter-spacing:.05em;padding:2px 7px;border-radius:999px}}
.pill-file{{background:var(--accent-file-dim);color:var(--accent-file)}}
.pill-function{{background:var(--accent-fn-dim);color:var(--accent-fn)}}
.pill-type{{background:var(--accent-type-dim);color:var(--accent-type)}}

/* ── graph canvas ────────────────────────────────────────────────────── */
#graph-canvas{{position:absolute;inset:0;cursor:grab}}
#graph-canvas:active{{cursor:grabbing}}
#tooltip{{position:fixed;pointer-events:none;background:rgba(13,17,23,.92);border:1px solid var(--border-hi);border-radius:9px;padding:8px 12px;font-size:12px;color:var(--text);white-space:nowrap;backdrop-filter:blur(8px);display:none;z-index:99;box-shadow:0 4px 24px rgba(0,0,0,.5)}}
.graph-legend{{position:absolute;bottom:18px;left:50%;transform:translateX(-50%);display:flex;gap:14px;background:rgba(13,17,23,.75);border:1px solid var(--border);border-radius:99px;padding:6px 18px;backdrop-filter:blur(8px);pointer-events:none}}
.legend-item{{display:flex;align-items:center;gap:5px;font-size:11px;color:var(--text-muted)}}
.legend-dot{{width:9px;height:9px;border-radius:50%}}

/* ── detail panel ────────────────────────────────────────────────────── */
.detail-placeholder{{display:flex;flex-direction:column;align-items:center;justify-content:center;height:100%;text-align:center;padding:32px;gap:10px;opacity:.55}}
.detail-placeholder svg{{opacity:.3}}
#details-body h2{{font-size:16px;font-weight:700;margin-bottom:8px}}
.detail-meta{{display:flex;align-items:center;gap:8px;margin-bottom:14px;flex-wrap:wrap}}
.detail-path{{font-size:11px;color:var(--text-muted);font-family:ui-monospace,monospace}}
.detail-section{{margin-bottom:16px}}
.detail-section h3{{font-size:11px;text-transform:uppercase;letter-spacing:.08em;color:var(--text-muted);margin-bottom:6px;font-weight:600}}
.doc-box{{white-space:pre-wrap;background:var(--surface2);border:1px solid var(--border);border-radius:10px;padding:11px 13px;line-height:1.55;font-family:ui-monospace,monospace;font-size:12px;color:#c9d1d9}}
.rel-list{{list-style:none;display:flex;flex-direction:column;gap:4px}}
.rel-item{{display:flex;align-items:center;gap:7px;padding:6px 9px;border-radius:8px;background:var(--surface2);border:1px solid var(--border);font-size:12px}}
.rel-arrow{{color:var(--text-muted);flex-shrink:0}}
.rel-label{{font-size:10px;text-transform:uppercase;letter-spacing:.05em;color:var(--text-muted)}}
</style>
</head>
<body>
<div id="app">

<!-- ─── LEFT SIDEBAR ──────────────────────────────────────────────────── -->
<aside id="sidebar-left">
  <div class="sidebar-header">
    <div class="logo">
      <div class="logo-icon">⬡</div>
      <span class="logo-text">Codebase Visualizer</span>
    </div>
    <div class="tagline">Explore functions, types &amp; relationships</div>
  </div>
  <div class="sidebar-body">
    <div class="stats" id="stats"></div>
    <div class="search-wrap">
      <span class="search-icon">⌕</span>
      <input id="search" placeholder="Search by name, path, docs…" autocomplete="off" spellcheck="false"/>
    </div>
    <select id="kind">
      <option value="all">All nodes</option>
      <option value="file">Files only</option>
      <option value="function">Functions only</option>
      <option value="type">Data structures only</option>
    </select>
    <div class="results-count" id="results-count"></div>
    <div class="item-list" id="results"></div>
  </div>
</aside>

<div class="handle" id="handle-left" title="Drag to resize"></div>

<!-- ─── GRAPH AREA ────────────────────────────────────────────────────── -->
<div id="graph-area">
  <canvas id="graph-canvas" aria-label="Codebase relationship graph"></canvas>
  <div id="tooltip"></div>
  <div class="graph-legend">
    <div class="legend-item"><div class="legend-dot" style="background:#3ddc84"></div>File</div>
    <div class="legend-item"><div class="legend-dot" style="background:#4da6ff"></div>Function</div>
    <div class="legend-item"><div class="legend-dot" style="background:#bf7bff"></div>Type</div>
  </div>
</div>

<div class="handle" id="handle-right" title="Drag to resize"></div>

<!-- ─── RIGHT SIDEBAR ────────────────────────────────────────────────── -->
<aside id="sidebar-right">
  <div class="sidebar-header" style="padding-bottom:14px">
    <div style="font-size:13px;font-weight:700;color:var(--text-muted);text-transform:uppercase;letter-spacing:.07em">Inspector</div>
  </div>
  <div class="sidebar-body" id="details-body">
    <div class="detail-placeholder">
      <svg width="48" height="48" viewBox="0 0 48 48" fill="none"><circle cx="24" cy="24" r="20" stroke="currentColor" stroke-width="2"/><circle cx="24" cy="24" r="7" stroke="currentColor" stroke-width="2"/><line x1="24" y1="4" x2="24" y2="17" stroke="currentColor" stroke-width="2"/><line x1="24" y1="31" x2="24" y2="44" stroke="currentColor" stroke-width="2"/><line x1="4" y1="24" x2="17" y2="24" stroke="currentColor" stroke-width="2"/><line x1="31" y1="24" x2="44" y2="24" stroke="currentColor" stroke-width="2"/></svg>
      <div style="font-weight:600;color:var(--text)">Select a node</div>
      <div style="font-size:12px">Click any node in the graph<br>or pick one from the list</div>
    </div>
  </div>
</aside>

</div><!-- #app -->
<div id="tooltip"></div>

<script>
/* ═══════════════════════════════════════════════════════════════════════
   DATA
═══════════════════════════════════════════════════════════════════════ */
const GRAPH = {graph_json};
const COLORS = {{ file:'#3ddc84', function:'#4da6ff', type:'#bf7bff' }};
const RADII  = {{ file:11, function:8, type:9 }};

/* ═══════════════════════════════════════════════════════════════════════
   STATE
═══════════════════════════════════════════════════════════════════════ */
const S = {{
  selected: null,
  visible: [],
  sim: [],        // simulation nodes with x,y,vx,vy
  edgesVis: [],
  panX: 0, panY: 0,
  zoom: 1,
  draggingNode: null,
  panning: false,
  lastMx: 0, lastMy: 0,
  hot: null,       // hovered node id
  dirty: true,
}};

/* ═══════════════════════════════════════════════════════════════════════
   DOM REFS
═══════════════════════════════════════════════════════════════════════ */
const searchEl  = document.getElementById('search');
const kindEl    = document.getElementById('kind');
const resultsCt = document.getElementById('results');
const countEl   = document.getElementById('results-count');
const detailsCt = document.getElementById('details-body');
const canvas    = document.getElementById('graph-canvas');
const tip       = document.getElementById('tooltip');
const ctx       = canvas.getContext('2d');

/* ═══════════════════════════════════════════════════════════════════════
   URL PARAMS
═══════════════════════════════════════════════════════════════════════ */
const params = new URLSearchParams(window.location.search);
const reqSelect = params.get('select');
if (params.get('q')) searchEl.value = params.get('q');
if (['all','file','function','type'].includes(params.get('kind'))) kindEl.value = params.get('kind');

/* ═══════════════════════════════════════════════════════════════════════
   STATS BAR
═══════════════════════════════════════════════════════════════════════ */
document.getElementById('stats').innerHTML = [
  ['file','file',GRAPH.stats.files||0,'Files'],
  ['fn','function',GRAPH.stats.functions||0,'Funcs'],
  ['type','type',GRAPH.stats.types||0,'Types'],
].map(([cls,_,v,l])=>`<div class="stat ${{cls}}"><div class="stat-value">${{v}}</div><div class="stat-label">${{l}}</div></div>`).join('');

/* ═══════════════════════════════════════════════════════════════════════
   FILTERING
═══════════════════════════════════════════════════════════════════════ */
function matches(node) {{
  const q = searchEl.value.trim().toLowerCase();
  const k = kindEl.value;
  if (k !== 'all' && node.kind !== k) return false;
  if (!q) return true;
  return (node.label+' '+node.path+' '+node.doc+' '+node.summary+' '+node.language).toLowerCase().includes(q);
}}

/* ═══════════════════════════════════════════════════════════════════════
   FORCE SIMULATION
═══════════════════════════════════════════════════════════════════════ */
const K_SPRING   = 0.018;
const K_REPEL    = 7000;
const K_DAMP     = 0.82;
const REST_LEN   = 140;
const CENTER_K   = 0.012;

function initSim() {{
  const W = canvas.width, H = canvas.height;
  const cx = W/2, cy = H/2;
  // Build node map for quick lookup
  const byId = new Map(S.sim.map(n=>[n.id,n]));
  S.sim = S.visible.map((node,i) => {{
    const old = byId.get(node.id);
    if (old) return {{ ...old, node }};
    const angle = (Math.PI*2*i/Math.max(S.visible.length,1));
    const r = 180 + Math.random()*120;
    return {{ id:node.id, node, x:cx+Math.cos(angle)*r, y:cy+Math.sin(angle)*r, vx:0, vy:0 }};
  }});
  const visIds = new Set(S.visible.map(n=>n.id));
  S.edgesVis = GRAPH.edges.filter(e=>visIds.has(e.source)&&visIds.has(e.target));
  S.dirty = true;
}}

function tick() {{
  if (S.draggingNode) return;
  const cx = canvas.width/2 + S.panX;
  const cy = canvas.height/2 + S.panY;
  let moving = false;

  // repulsion between all pairs
  for (let i=0;i<S.sim.length;i++) {{
    const a = S.sim[i];
    for (let j=i+1;j<S.sim.length;j++) {{
      const b = S.sim[j];
      const dx=a.x-b.x, dy=a.y-b.y;
      const dist2 = dx*dx+dy*dy+1;
      const force = K_REPEL/dist2;
      const fx=force*dx, fy=force*dy;
      a.vx+=fx; a.vy+=fy;
      b.vx-=fx; b.vy-=fy;
    }}
  }}

  // spring attraction along edges
  const posMap = new Map(S.sim.map(n=>[n.id,n]));
  for (const e of S.edgesVis) {{
    const a=posMap.get(e.source), b=posMap.get(e.target);
    if (!a||!b) continue;
    const dx=b.x-a.x, dy=b.y-a.y;
    const dist=Math.sqrt(dx*dx+dy*dy)+0.01;
    const force=K_SPRING*(dist-REST_LEN);
    const fx=force*dx/dist, fy=force*dy/dist;
    a.vx+=fx; a.vy+=fy;
    b.vx-=fx; b.vy-=fy;
  }}

  // center gravity
  for (const n of S.sim) {{
    n.vx += (cx - n.x)*CENTER_K;
    n.vy += (cy - n.y)*CENTER_K;
    n.vx *= K_DAMP; n.vy *= K_DAMP;
    n.x += n.vx; n.y += n.vy;
    if (Math.abs(n.vx)>0.05||Math.abs(n.vy)>0.05) moving=true;
  }}
  if (moving) S.dirty=true;
}}

/* ═══════════════════════════════════════════════════════════════════════
   CANVAS DRAWING
═══════════════════════════════════════════════════════════════════════ */
function draw() {{
  const W=canvas.width, H=canvas.height;
  ctx.clearRect(0,0,W,H);

  // background grid
  ctx.save();
  ctx.strokeStyle='rgba(255,255,255,.028)';
  ctx.lineWidth=1;
  const gStep=50;
  for (let x=0;x<W;x+=gStep) {{ ctx.beginPath();ctx.moveTo(x,0);ctx.lineTo(x,H);ctx.stroke(); }}
  for (let y=0;y<H;y+=gStep) {{ ctx.beginPath();ctx.moveTo(0,y);ctx.lineTo(W,y);ctx.stroke(); }}
  ctx.restore();

  const posMap = new Map(S.sim.map(n=>[n.id,n]));

  // draw edges
  for (const e of S.edgesVis) {{
    const a=posMap.get(e.source), b=posMap.get(e.target);
    if (!a||!b) continue;
    const isRelated = S.selected && (e.source===S.selected||e.target===S.selected);
    ctx.beginPath();
    ctx.moveTo(a.x,a.y);
    ctx.lineTo(b.x,b.y);
    if (isRelated) {{
      ctx.strokeStyle='rgba(99,179,255,.55)';
      ctx.lineWidth=1.8;
    }} else {{
      ctx.strokeStyle='rgba(255,255,255,.07)';
      ctx.lineWidth=1;
    }}
    ctx.stroke();
  }}

  // draw nodes
  for (const n of S.sim) {{
    const r = RADII[n.node.kind]||8;
    const col = COLORS[n.node.kind]||'#aaa';
    const isSelected = n.id===S.selected;
    const isHot = n.id===S.hot;
    const isRelated = S.selected && S.edgesVis.some(e=>(e.source===S.selected&&e.target===n.id)||(e.target===S.selected&&e.source===n.id));

    ctx.save();

    // outer glow
    if (isSelected||isHot) {{
      ctx.shadowColor = col;
      ctx.shadowBlur  = isSelected ? 28 : 14;
    }} else if (isRelated) {{
      ctx.shadowColor = col;
      ctx.shadowBlur  = 10;
    }}

    // ring for selected
    if (isSelected) {{
      ctx.beginPath();
      ctx.arc(n.x,n.y,r+5,0,Math.PI*2);
      ctx.strokeStyle=col;
      ctx.globalAlpha=0.45;
      ctx.lineWidth=2;
      ctx.stroke();
      ctx.globalAlpha=1;
    }}

    // filled circle
    ctx.beginPath();
    ctx.arc(n.x,n.y,r,0,Math.PI*2);
    if (isSelected||isHot) {{
      ctx.fillStyle=col;
    }} else if (isRelated) {{
      ctx.fillStyle=col;
      ctx.globalAlpha=0.75;
    }} else {{
      // dim unrelated nodes when something is selected
      ctx.globalAlpha = S.selected ? 0.35 : 1;
      const grad=ctx.createRadialGradient(n.x-r*.25,n.y-r*.25,r*.1,n.x,n.y,r);
      grad.addColorStop(0,col);
      grad.addColorStop(1,col+'88');
      ctx.fillStyle=grad;
    }}
    ctx.fill();
    ctx.globalAlpha=1;
    ctx.shadowBlur=0;

    // label
    ctx.font=`${{(isSelected||isHot)?'600 ':'400 '}}11px Inter,sans-serif`;
    ctx.fillStyle = (S.selected && !isSelected && !isRelated) ? 'rgba(226,232,244,.3)' : '#e2e8f4';
    ctx.textBaseline='middle';
    ctx.fillText(n.node.label, n.x+r+5, n.y);
    ctx.restore();
  }}
}}

/* ═══════════════════════════════════════════════════════════════════════
   ANIMATION LOOP
═══════════════════════════════════════════════════════════════════════ */
function loop() {{
  tick();
  if (S.dirty) {{ draw(); S.dirty=false; }}
  requestAnimationFrame(loop);
}}

/* ═══════════════════════════════════════════════════════════════════════
   CANVAS RESIZE
═══════════════════════════════════════════════════════════════════════ */
function resizeCanvas() {{
  const area = document.getElementById('graph-area');
  canvas.width  = area.clientWidth;
  canvas.height = area.clientHeight;
  S.dirty = true;
}}

/* ═══════════════════════════════════════════════════════════════════════
   HIT TEST
═══════════════════════════════════════════════════════════════════════ */
function hitTest(mx,my) {{
  for (const n of S.sim) {{
    const r=RADII[n.node.kind]||8;
    const dx=n.x-mx, dy=n.y-my;
    if (dx*dx+dy*dy <= (r+4)*(r+4)) return n;
  }}
  return null;
}}

/* ═══════════════════════════════════════════════════════════════════════
   POINTER EVENTS (canvas)
═══════════════════════════════════════════════════════════════════════ */
canvas.addEventListener('mousedown', e=>{{
  const n=hitTest(e.offsetX,e.offsetY);
  if (n) {{
    S.draggingNode=n;
    selectNode(n.id);
  }} else {{
    S.panning=true;
    S.lastMx=e.clientX; S.lastMy=e.clientY;
  }}
}});

canvas.addEventListener('mousemove', e=>{{
  const n=hitTest(e.offsetX,e.offsetY);
  const newHot = n?n.id:null;
  if (newHot!==S.hot) {{ S.hot=newHot; S.dirty=true; }}

  if (S.draggingNode) {{
    S.draggingNode.x=e.offsetX;
    S.draggingNode.y=e.offsetY;
    S.draggingNode.vx=0; S.draggingNode.vy=0;
    S.dirty=true;
  }} else if (S.panning) {{
    const dx=e.clientX-S.lastMx, dy=e.clientY-S.lastMy;
    for (const n of S.sim) {{ n.x+=dx; n.y+=dy; }}
    S.lastMx=e.clientX; S.lastMy=e.clientY;
    S.dirty=true;
  }}

  if (n) {{
    tip.style.display='block';
    tip.style.left=(e.clientX+14)+'px';
    tip.style.top=(e.clientY-8)+'px';
    tip.textContent=`${{n.node.kind.toUpperCase()}}  ${{n.node.label}}  ·  ${{n.node.path}}:${{n.node.line}}`;
  }} else {{
    tip.style.display='none';
  }}
}});

window.addEventListener('mouseup', ()=>{{
  S.draggingNode=null;
  S.panning=false;
}});

canvas.addEventListener('wheel', e=>{{
  e.preventDefault();
  const factor = e.deltaY < 0 ? 1.1 : 0.91;
  const cx=e.offsetX, cy=e.offsetY;
  for (const n of S.sim) {{
    n.x = cx + (n.x-cx)*factor;
    n.y = cy + (n.y-cy)*factor;
  }}
  S.dirty=true;
}}, {{passive:false}});

/* ═══════════════════════════════════════════════════════════════════════
   SELECT NODE
═══════════════════════════════════════════════════════════════════════ */
function selectNode(id) {{
  S.selected = id;
  S.dirty = true;
  renderDetails();
  renderResults();
}}

/* ═══════════════════════════════════════════════════════════════════════
   SIDEBAR LIST
═══════════════════════════════════════════════════════════════════════ */
function pillClass(kind) {{
  return kind==='file'?'pill-file':kind==='function'?'pill-function':'pill-type';
}}
function activeClass(node) {{
  if (S.selected!==node.id) return '';
  return node.kind==='file'?'active-file':node.kind==='type'?'active-type':'active';
}}

function renderResults() {{
  const visible = S.visible.slice(0,100);
  const total = S.visible.length;
  countEl.textContent = total===0?'No matches':`${{total}} node${{total===1?'':'s'}}${{total>100?' (showing 100)':''}}`;
  resultsCt.innerHTML = visible.map(node=>`
    <button class="item ${{activeClass(node)}}" data-id="${{node.id}}">
      <div class="item-top">
        <span class="pill ${{pillClass(node.kind)}}">${{node.kind}}</span>
        <span class="item-name">${{escapeHtml(node.label)}}</span>
      </div>
      <div class="item-path">${{escapeHtml(node.path)}}:${{node.line}}</div>
      <div class="item-summary">${{escapeHtml(node.summary)}}</div>
    </button>
  `).join('');
  resultsCt.querySelectorAll('button').forEach(b=>b.addEventListener('click',()=>selectNode(b.dataset.id)));
}}

/* ═══════════════════════════════════════════════════════════════════════
   DETAIL PANEL
═══════════════════════════════════════════════════════════════════════ */
function renderDetails() {{
  const id = S.selected;
  const node = GRAPH.nodes.find(n=>n.id===id);
  if (!node) {{
    detailsCt.innerHTML=`<div class="detail-placeholder">
      <svg width="48" height="48" viewBox="0 0 48 48" fill="none"><circle cx="24" cy="24" r="20" stroke="currentColor" stroke-width="2"/><circle cx="24" cy="24" r="7" stroke="currentColor" stroke-width="2"/><line x1="24" y1="4" x2="24" y2="17" stroke="currentColor" stroke-width="2"/><line x1="24" y1="31" x2="24" y2="44" stroke="currentColor" stroke-width="2"/><line x1="4" y1="24" x2="17" y2="24" stroke="currentColor" stroke-width="2"/><line x1="31" y1="24" x2="44" y2="24" stroke="currentColor" stroke-width="2"/></svg>
      <div style="font-weight:600;color:var(--text)">Select a node</div>
      <div style="font-size:12px">Click any node in the graph<br>or pick one from the list</div>
    </div>`;
    return;
  }}
  const related = GRAPH.edges.filter(e=>e.source===id||e.target===id).slice(0,15);
  const accentColor = COLORS[node.kind]||'#aaa';
  detailsCt.innerHTML=`
    <div style="padding-bottom:14px;margin-bottom:14px;border-bottom:1px solid var(--border)">
      <div style="display:flex;align-items:center;gap:8px;margin-bottom:6px">
        <span style="width:10px;height:10px;border-radius:50%;background:${{accentColor}};flex-shrink:0;box-shadow:0 0 8px ${{accentColor}}"></span>
        <span class="pill ${{pillClass(node.kind)}}">${{node.kind}}</span>
        <span style="font-size:11px;color:var(--text-muted)">${{escapeHtml(node.language)}}</span>
      </div>
      <h2 style="font-size:16px;font-weight:700;word-break:break-all">${{escapeHtml(node.label)}}</h2>
      <div class="detail-path">${{escapeHtml(node.path)}}:${{node.line}}</div>
    </div>

    <div class="detail-section">
      <h3>Summary</h3>
      <p style="font-size:13px">${{escapeHtml(node.summary)}}</p>
    </div>

    <div class="detail-section">
      <h3>Documentation</h3>
      <div class="doc-box">${{escapeHtml(node.doc||'No nearby documentation comment found.')}}</div>
    </div>

    <div class="detail-section">
      <h3>Relationships <span style="font-weight:400;color:var(--text-muted)">(${{related.length}})</span></h3>
      ${{related.length===0
        ? '<p style="font-size:12px;color:var(--text-muted)">No visible relationships.</p>'
        : `<ul class="rel-list">${{related.map(e=>{{
            const other = e.source===id ? e.target : e.source;
            const otherNode = GRAPH.nodes.find(n=>n.id===other);
            const arrow = e.source===id ? '→' : '←';
            return `<li class="rel-item">
              <span class="rel-label">${{escapeHtml(e.label)}}</span>
              <span class="rel-arrow">${{arrow}}</span>
              <span style="font-size:12px;color:var(--text)">${{escapeHtml(otherNode?.label||other)}}</span>
              ${{otherNode ? `<span style="margin-left:auto"><span class="pill ${{pillClass(otherNode.kind)}}">${{otherNode.kind}}</span></span>` : ''}}
            </li>`;
          }}).join('')}}</ul>`
      }}
    </div>
  `;
  detailsCt.scrollTop=0;
}}

/* ═══════════════════════════════════════════════════════════════════════
   FULL RENDER (filter changed)
═══════════════════════════════════════════════════════════════════════ */
function fullRender() {{
  S.visible = GRAPH.nodes.filter(matches);
  if (S.selected && !S.visible.some(n=>n.id===S.selected)) S.selected=null;
  // auto-select from URL param on first render
  if (!S.selected && reqSelect) {{
    const req=reqSelect.toLowerCase();
    const hit=S.visible.find(n=>n.label.toLowerCase()===req)||S.visible.find(n=>n.id.toLowerCase().includes(req));
    if (hit) S.selected=hit.id;
  }}
  if (!S.selected && S.visible.length===1) S.selected=S.visible[0].id;
  initSim();
  renderResults();
  renderDetails();
}}

/* ═══════════════════════════════════════════════════════════════════════
   RESIZE HANDLES (drag)
═══════════════════════════════════════════════════════════════════════ */
function makeHandle(handleEl, sidebarEl, side) {{
  let dragging=false, startX=0, startW=0;
  handleEl.addEventListener('mousedown', e=>{{
    dragging=true;
    startX=e.clientX;
    startW=sidebarEl.offsetWidth;
    handleEl.classList.add('dragging');
    document.body.style.userSelect='none';
    document.body.style.cursor='col-resize';
  }});
  window.addEventListener('mousemove', e=>{{
    if (!dragging) return;
    const delta = side==='left' ? e.clientX-startX : startX-e.clientX;
    const newW = Math.max(160, Math.min(520, startW+delta));
    sidebarEl.style.width=newW+'px';
    resizeCanvas();
    S.dirty=true;
  }});
  window.addEventListener('mouseup', ()=>{{
    if (!dragging) return;
    dragging=false;
    handleEl.classList.remove('dragging');
    document.body.style.userSelect='';
    document.body.style.cursor='';
  }});
}}
makeHandle(document.getElementById('handle-left'),  document.getElementById('sidebar-left'),  'left');
makeHandle(document.getElementById('handle-right'), document.getElementById('sidebar-right'), 'right');

/* ═══════════════════════════════════════════════════════════════════════
   HELPERS
═══════════════════════════════════════════════════════════════════════ */
function escapeHtml(v) {{
  return String(v).replace(/[&<>"']/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#039;'}}[c]));
}}

/* ═══════════════════════════════════════════════════════════════════════
   BOOT
═══════════════════════════════════════════════════════════════════════ */
const ro = new ResizeObserver(()=>{{ resizeCanvas(); S.dirty=true; }});
ro.observe(document.getElementById('graph-area'));
resizeCanvas();

searchEl.addEventListener('input',  fullRender);
kindEl.addEventListener('change',   fullRender);

fullRender();
loop();
</script>
</body>
</html>"#
    )
}

fn graph_to_json(graph: &Graph) -> String {
    let stats = graph
        .stats
        .iter()
        .map(|(key, value)| format!("{}:{value}", json_string(key)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"root\":{},\"nodes\":[{}],\"edges\":[{}],\"stats\":{{{stats}}}}}",
        json_string(&graph.root),
        graph
            .nodes
            .iter()
            .map(node_to_json)
            .collect::<Vec<_>>()
            .join(","),
        graph
            .edges
            .iter()
            .map(edge_to_json)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn node_to_json(node: &Node) -> String {
    format!(
        "{{\"id\":{},\"label\":{},\"kind\":{},\"path\":{},\"line\":{},\"language\":{},\"doc\":{},\"summary\":{}}}",
        json_string(&node.id),
        json_string(&node.label),
        json_string(kind_label(&node.kind)),
        json_string(&node.path),
        node.line,
        json_string(&node.language),
        json_string(&node.doc),
        json_string(&node.summary)
    )
}

fn edge_to_json(edge: &Edge) -> String {
    format!(
        "{{\"source\":{},\"target\":{},\"label\":{}}}",
        json_string(&edge.source),
        json_string(&edge.target),
        json_string(&edge.label)
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn source_mentions(source: &str, symbol: &str) -> bool {
    source
        .split(|ch: char| !is_identifier_char(ch))
        .any(|part| part == symbol)
}

fn symbol_after_keywords(line: &str, allowed_prefixes: &[&str], keyword: &str) -> Option<String> {
    let mut tokens = line.split_whitespace();
    let mut saw_keyword = false;
    while let Some(token) = tokens.next() {
        let clean = token.trim_matches(|ch: char| !is_identifier_char(ch));
        if clean == keyword {
            saw_keyword = true;
            break;
        }
        if !allowed_prefixes.contains(&clean) {
            return None;
        }
    }
    if saw_keyword {
        tokens.next().and_then(first_identifier)
    } else {
        None
    }
}

fn symbol_after_any_keyword(line: &str, keywords: &[&str]) -> Option<String> {
    let line = line.strip_prefix("export ").unwrap_or(line);
    for keyword in keywords {
        let Some(index) = line.find(keyword) else {
            continue;
        };
        let before = &line[..index];
        let after = &line[index + keyword.len()..];
        let keyword_is_standalone = before
            .chars()
            .last()
            .map_or(true, |ch| !is_identifier_char(ch))
            && after
                .chars()
                .next()
                .map_or(true, |ch| !is_identifier_char(ch));
        if keyword_is_standalone {
            return first_identifier(after.trim_start());
        }
    }
    None
}

fn capture_js_function(line: &str) -> Option<String> {
    let line = line.strip_prefix("export ").unwrap_or(line);
    let line = line.strip_prefix("async ").unwrap_or(line);
    if let Some(rest) = line.strip_prefix("function ") {
        return first_identifier(rest);
    }
    if line.contains("=>")
        || line.contains("function")
        || line.contains("= async (")
        || line.contains("= (")
    {
        return name_before_assignment(line);
    }
    None
}

fn capture_go_function(line: &str) -> Option<String> {
    let rest = line.strip_prefix("func ")?;
    let rest = if rest.starts_with('(') {
        rest.split_once(')').map(|(_, after)| after.trim_start())?
    } else {
        rest
    };
    first_identifier(rest)
}

fn capture_java_like_function(line: &str) -> Option<String> {
    if !line.contains('(')
        || line.ends_with(';')
        || symbol_after_any_keyword(line, &["if", "for", "while", "switch"]).is_some()
    {
        return None;
    }
    name_before_open_paren(line).filter(|name| {
        !matches!(
            name.as_str(),
            "if" | "for" | "while" | "switch" | "catch" | "return"
        )
    })
}

fn first_identifier(value: &str) -> Option<String> {
    let mut chars = value.chars();
    let first = chars.next()?;
    if !is_identifier_start(first) {
        return None;
    }
    let mut name = String::from(first);
    for ch in chars {
        if is_identifier_char(ch) {
            name.push(ch);
        } else {
            break;
        }
    }
    Some(name)
}

fn name_before_assignment(line: &str) -> Option<String> {
    let left = line.split('=').next()?;
    let mut parts = left.split_whitespace();
    let keyword = parts.next()?;
    if !matches!(keyword, "const" | "let" | "var") {
        return None;
    }
    first_identifier(parts.next()?)
}

fn name_before_open_paren(line: &str) -> Option<String> {
    let before_paren = line.split('(').next()?.trim_end();
    before_paren
        .split(|ch: char| !is_identifier_char(ch))
        .filter(|part| !part.is_empty())
        .last()
        .map(ToString::to_string)
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn node_id(kind: &str, path: &str, line: usize, label: &str) -> String {
    format!("{kind}:{path}:{line}:{label}")
}

fn kind_label(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::File => "file",
        NodeKind::Function => "function",
        NodeKind::Type => "type",
    }
}

fn first_sentence(text: &str) -> String {
    text.split('.')
        .next()
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .unwrap_or(text)
        .to_string()
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphabetic()
}

fn is_identifier_char(ch: char) -> bool {
    is_identifier_start(ch) || ch.is_ascii_digit()
}

fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "if" | "for" | "while" | "switch" | "catch" | "return" | "new" | "match" | "fn"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn extracts_rust_docs_functions_and_types() {
        let patterns = patterns_for("rust").unwrap();
        let nodes = extract_symbols(
            "/// Stores graph nodes.\npub struct GraphStore {}\n\n/// Builds the graph.\npub fn build_graph() {}\n",
            "src/lib.rs",
            "rust",
            patterns,
        );

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].label, "GraphStore");
        assert_eq!(nodes[0].summary, "Stores graph nodes");
        assert_eq!(nodes[1].label, "build_graph");
        assert_eq!(nodes[1].summary, "Builds the graph");
    }

    #[test]
    fn renders_html_with_embedded_graph_json() {
        let graph = Graph {
            root: "sample".to_string(),
            nodes: vec![Node {
                id: "function:src/lib.rs:1:run".to_string(),
                label: "run".to_string(),
                kind: NodeKind::Function,
                path: "src/lib.rs".to_string(),
                line: 1,
                language: "rust".to_string(),
                doc: "Runs the app.".to_string(),
                summary: "Runs the app".to_string(),
            }],
            edges: Vec::new(),
            stats: BTreeMap::from([("functions".to_string(), 1)]),
        };

        let html = render_html(&graph);

        assert!(html.contains("Codebase Visualizer"));
        assert!(html.contains("\"label\":\"run\""));
        assert!(html.contains("graph-canvas"));
        assert!(html.contains("sidebar-left"));
        assert!(html.contains("handle-left"));
        assert!(!html.contains("applyInitialUrlState"));
    }

    #[test]
    fn scans_a_small_mixed_codebase() -> Result<()> {
        let root = unique_temp_dir()?;
        fs::create_dir_all(root.join("src"))?;
        fs::write(
            root.join("src/main.rs"),
            "/// Entry point.\nfn main() { render(); }\n\n/// Draws the UI.\nfn render() {}\n",
        )?;
        fs::write(
            root.join("src/app.py"),
            "# Represents a task.\nclass Task:\n    pass\n\n# Loads tasks.\ndef load_tasks():\n    return Task()\n",
        )?;

        let graph = scan_codebase(&root, 500_000)?;
        fs::remove_dir_all(root)?;

        assert_eq!(graph.stats.get("files"), Some(&2));
        assert_eq!(graph.stats.get("functions"), Some(&3));
        assert_eq!(graph.stats.get("types"), Some(&1));
        assert!(graph.edges.iter().any(|edge| edge.label == "mentions"));
        Ok(())
    }

    fn unique_temp_dir() -> Result<PathBuf> {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_nanos()
            .to_string();
        let dir = env::temp_dir().join(format!("codebase_visualizer_test_{suffix}"));
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}
