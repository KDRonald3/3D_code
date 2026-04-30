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
    let escaped_title = escape_html(&format!("Codebase map: {}", graph.root));

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>{escaped_title}</title>
<style>
:root {{ color-scheme: dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; }}
body {{ margin: 0; background: #0d1117; color: #e6edf3; }}
.app {{ display: grid; grid-template-columns: 360px 1fr 340px; min-height: 100vh; }}
aside, main {{ border-right: 1px solid #30363d; }}
aside, .details {{ padding: 18px; overflow: auto; }}
h1 {{ font-size: 22px; margin: 0 0 8px; }}
.muted {{ color: #8b949e; font-size: 13px; }}
.stats {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; margin: 18px 0; }}
.stat {{ background: #161b22; border: 1px solid #30363d; border-radius: 12px; padding: 10px; }}
.stat strong {{ display: block; font-size: 24px; }}
input, select {{ width: 100%; box-sizing: border-box; margin: 8px 0; padding: 10px 12px; border-radius: 10px; border: 1px solid #30363d; color: #e6edf3; background: #010409; }}
.list {{ display: grid; gap: 8px; margin-top: 12px; }}
button.item {{ text-align: left; border: 1px solid #30363d; background: #161b22; color: #e6edf3; padding: 10px; border-radius: 10px; cursor: pointer; }}
button.item:hover, button.item.active {{ border-color: #58a6ff; background: #0b2442; }}
.pill {{ display: inline-block; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; padding: 2px 7px; border-radius: 999px; background: #21262d; color: #a5d6ff; }}
main {{ position: relative; overflow: hidden; }}
svg {{ width: 100%; height: 100vh; display: block; background: radial-gradient(circle at top left, #172033, #0d1117 42%); }}
line {{ stroke: #41546f; stroke-width: 1.4; opacity: .55; }}
circle {{ stroke: #e6edf3; stroke-width: 1.5; cursor: pointer; }}
text {{ fill: #e6edf3; font-size: 12px; pointer-events: none; text-shadow: 0 1px 4px #010409; }}
.details h2 {{ margin-top: 0; }}
.doc {{ white-space: pre-wrap; background: #161b22; border: 1px solid #30363d; border-radius: 12px; padding: 12px; line-height: 1.45; }}
</style>
</head>
<body>
<div class="app">
<aside>
<h1>Codebase Visualizer</h1>
<div class="muted">Search functions, data structures, files, and doc-derived summaries.</div>
<div class="stats" id="stats"></div>
<input id="search" placeholder="Search label, path, docs..." autofocus />
<select id="kind">
<option value="all">All nodes</option>
<option value="file">Files</option>
<option value="function">Functions</option>
<option value="type">Data structures</option>
</select>
<div class="list" id="results"></div>
</aside>
<main><svg id="graph" role="img" aria-label="Codebase relationship graph"></svg></main>
<section class="details" id="details">
<h2>Select a node</h2>
<p class="muted">Click a graph node or search result to inspect its summary, documentation, path, and relationships.</p>
</section>
</div>
<script>
const graph = {graph_json};
const state = {{ selected: null, visible: graph.nodes }};
const colors = {{ file: '#7ee787', function: '#58a6ff', type: '#d2a8ff' }};
const radii = {{ file: 10, function: 7, type: 8 }};
const search = document.getElementById('search');
const kind = document.getElementById('kind');
const results = document.getElementById('results');
const details = document.getElementById('details');
const svg = document.getElementById('graph');

document.getElementById('stats').innerHTML = [
  ['Files', graph.stats.files || 0],
  ['Funcs', graph.stats.functions || 0],
  ['Types', graph.stats.types || 0],
].map(([label, value]) => `<div class="stat"><strong>${{value}}</strong><span>${{label}}</span></div>`).join('');

function matches(node) {{
  const q = search.value.trim().toLowerCase();
  const kindValue = kind.value;
  const haystack = `${{node.label}} ${{node.path}} ${{node.doc}} ${{node.summary}} ${{node.language}}`.toLowerCase();
  return (kindValue === 'all' || node.kind === kindValue) && (!q || haystack.includes(q));
}}

function layout(nodes) {{
  const width = svg.clientWidth || 900;
  const height = svg.clientHeight || 700;
  const cx = width / 2;
  const cy = height / 2;
  const rings = {{ file: Math.min(width, height) * .18, type: Math.min(width, height) * .30, function: Math.min(width, height) * .41 }};
  const byKind = {{ file: [], type: [], function: [] }};
  nodes.forEach(node => byKind[node.kind].push(node));
  for (const group of Object.values(byKind)) {{
    group.forEach((node, index) => {{
      const angle = (Math.PI * 2 * index / Math.max(group.length, 1)) - Math.PI / 2;
      const radius = rings[node.kind] || rings.function;
      node.x = cx + Math.cos(angle) * radius;
      node.y = cy + Math.sin(angle) * radius;
    }});
  }}
}}

function render() {{
  state.visible = graph.nodes.filter(matches);
  const visibleIds = new Set(state.visible.map(node => node.id));
  layout(state.visible);
  const positions = new Map(state.visible.map(node => [node.id, node]));
  const edges = graph.edges.filter(edge => visibleIds.has(edge.source) && visibleIds.has(edge.target));

  svg.innerHTML = edges.map(edge => {{
    const source = positions.get(edge.source);
    const target = positions.get(edge.target);
    return `<line x1="${{source.x}}" y1="${{source.y}}" x2="${{target.x}}" y2="${{target.y}}"></line>`;
  }}).join('') + state.visible.map(node => `
    <g data-id="${{node.id}}">
      <circle cx="${{node.x}}" cy="${{node.y}}" r="${{radii[node.kind] || 7}}" fill="${{colors[node.kind]}}"></circle>
      <text x="${{node.x + 11}}" y="${{node.y + 4}}">${{escapeHtml(node.label)}}</text>
    </g>
  `).join('');

  svg.querySelectorAll('g').forEach(group => group.addEventListener('click', () => selectNode(group.dataset.id)));
  renderResults();
}}

function renderResults() {{
  results.innerHTML = state.visible.slice(0, 80).map(node => `
    <button class="item ${{state.selected === node.id ? 'active' : ''}}" data-id="${{node.id}}">
      <span class="pill">${{node.kind}}</span>
      <strong>${{escapeHtml(node.label)}}</strong>
      <div class="muted">${{escapeHtml(node.path)}}:${{node.line}}</div>
      <div>${{escapeHtml(node.summary)}}</div>
    </button>
  `).join('');
  results.querySelectorAll('button').forEach(button => button.addEventListener('click', () => selectNode(button.dataset.id)));
}}

function selectNode(id) {{
  state.selected = id;
  const node = graph.nodes.find(candidate => candidate.id === id);
  const related = graph.edges.filter(edge => edge.source === id || edge.target === id).slice(0, 20);
  details.innerHTML = `
    <h2>${{escapeHtml(node.label)}}</h2>
    <p><span class="pill">${{node.kind}}</span> <span class="muted">${{escapeHtml(node.language)}} - ${{escapeHtml(node.path)}}:${{node.line}}</span></p>
    <h3>Summary</h3><p>${{escapeHtml(node.summary)}}</p>
    <h3>Documentation</h3><div class="doc">${{escapeHtml(node.doc || 'No nearby documentation comment found.')}}</div>
    <h3>Relationships</h3>
    <ul>${{related.map(edge => `<li>${{escapeHtml(edge.label)}}: ${{escapeHtml(labelFor(edge.source))}} -> ${{escapeHtml(labelFor(edge.target))}}</li>`).join('') || '<li>No visible relationships.</li>'}}</ul>
  `;
  renderResults();
}}

function labelFor(id) {{
  return graph.nodes.find(node => node.id === id)?.label || id;
}}

function escapeHtml(value) {{
  return String(value).replace(/[&<>"']/g, ch => ({{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#039;'}}[ch]));
}}

search.addEventListener('input', render);
kind.addEventListener('change', render);
window.addEventListener('resize', render);
render();
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
        assert!(html.contains("Search functions"));
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
