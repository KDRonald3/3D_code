use std::collections::{BTreeMap, HashMap};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::{Node as TsNode, Parser as TsParser};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct Args {
    input: PathBuf,
    output: PathBuf,
    max_file_bytes: u64,
    embed_libs: bool,
}

#[derive(Debug, Clone)]
struct Node {
    id: String,
    label: String,
    kind: NodeKind,
    path: String,
    line: usize,
    end_line: usize,
    language: String,
    doc: String,
    summary: String,
    code: String,
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
    kind: EdgeKind,
    label: String,
    callsites: Vec<Callsite>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EdgeKind {
    Contains,
    Calls,
}

#[derive(Debug, Clone)]
struct Callsite {
    path: String,
    line: usize,
    col: usize,
    snippet: String,
    caller: String,
}

#[derive(Debug)]
struct Graph {
    root: String,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    stats: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
struct RustFunctionDef {
    id: String,
    name: String,
    path: String,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    end_line: usize,
    code: String,
    doc: String,
}

#[derive(Debug, Clone)]
struct RustTypeDef {
    id: String,
    name: String,
    path: String,
    start_line: usize,
    end_line: usize,
    doc: String,
}

#[derive(Debug, Clone)]
struct RustCall {
    caller_id: String,
    callee_name: String,
    path: String,
    byte: usize,
    snippet: String,
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
    let html = render_html(&graph, args.embed_libs);

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
    let mut embed_libs = false;
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
            "--embed-libs" => {
                embed_libs = true;
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
        embed_libs,
    })
}

fn print_help() {
    println!(
        "codebase-visualizer\n\nUSAGE:\n    codebase_visualizer [INPUT] [--output FILE] [--max-file-bytes BYTES] [--embed-libs]\n\nGenerates a self-contained searchable 3D HTML call graph of Rust functions (plus a types view)."
    );
}

fn scan_codebase(root: &Path, max_file_bytes: u64) -> Result<Graph> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut scanned_files = Vec::new();
    let mut rust_functions: Vec<RustFunctionDef> = Vec::new();
    let mut rust_types: Vec<RustTypeDef> = Vec::new();
    let mut rust_calls: Vec<RustCall> = Vec::new();
    let mut parser = rust_parser()?;

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
            end_line: source.lines().count().max(1),
            language: language.clone(),
            doc: String::new(),
            summary: summarize_file(&source),
            code: String::new(),
        };

        scanned_files.push(ScannedFile {
            node: file_node.clone(),
            source: source.clone(),
        });
        nodes.push(file_node);

        if language == "rust" {
            let (defs, type_defs, calls) =
                parse_rust_symbols_and_calls(&mut parser, &source, &relative_path)?;
            rust_functions.extend(defs);
            rust_types.extend(type_defs);
            rust_calls.extend(calls);
        }
    }

    // Add Rust function/type nodes and file->symbol containment edges.
    for def in &rust_functions {
        let node = Node {
            id: def.id.clone(),
            label: def.name.clone(),
            kind: NodeKind::Function,
            path: def.path.clone(),
            line: def.start_line,
            end_line: def.end_line,
            language: "rust".to_string(),
            doc: def.doc.clone(),
            summary: format!("fn {} (lines {}-{})", def.name, def.start_line, def.end_line),
            code: def.code.clone(),
        };
        let file_id = node_id("file", &def.path, 0, &def.path);
        edges.push(Edge {
            source: file_id,
            target: node.id.clone(),
            kind: EdgeKind::Contains,
            label: "contains".to_string(),
            callsites: Vec::new(),
        });
        nodes.push(node);
    }

    for def in &rust_types {
        let node = Node {
            id: def.id.clone(),
            label: def.name.clone(),
            kind: NodeKind::Type,
            path: def.path.clone(),
            line: def.start_line,
            end_line: def.end_line,
            language: "rust".to_string(),
            doc: def.doc.clone(),
            summary: format!("type {} (lines {}-{})", def.name, def.start_line, def.end_line),
            code: String::new(),
        };
        let file_id = node_id("file", &def.path, 0, &def.path);
        edges.push(Edge {
            source: file_id,
            target: node.id.clone(),
            kind: EdgeKind::Contains,
            label: "contains".to_string(),
            callsites: Vec::new(),
        });
        nodes.push(node);
    }

    edges.extend(build_rust_call_edges(&nodes, &scanned_files, &rust_calls));

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
    stats.insert(
        "calls".to_string(),
        edges.iter().filter(|edge| edge.kind == EdgeKind::Calls).count(),
    );

    Ok(Graph {
        root: root.display().to_string(),
        nodes,
        edges,
        stats,
    })
}

fn rust_parser() -> Result<TsParser> {
    let mut parser = TsParser::new();
    parser
        .set_language(&tree_sitter_rust::language())
        .map_err(|_| "failed to load tree-sitter-rust grammar".to_string())?;
    Ok(parser)
}

fn parse_rust_symbols_and_calls(
    parser: &mut TsParser,
    source: &str,
    path: &str,
) -> Result<(Vec<RustFunctionDef>, Vec<RustTypeDef>, Vec<RustCall>)> {
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "failed to parse rust source".to_string())?;
    let root = tree.root_node();

    let mut funcs = Vec::new();
    let mut types = Vec::new();
    let mut calls = Vec::new();

    collect_rust_defs(root, source, path, &mut funcs, &mut types);
    collect_rust_calls(root, source, path, &funcs, &mut calls);

    Ok((funcs, types, calls))
}

fn collect_rust_defs(
    node: TsNode,
    source: &str,
    path: &str,
    funcs: &mut Vec<RustFunctionDef>,
    types: &mut Vec<RustTypeDef>,
) {
    let kind = node.kind();
    if kind == "function_item" {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                let start_byte = node.start_byte();
                let end_byte = node.end_byte();
                let (start_line, _) = byte_to_line_col(source, start_byte);
                let (end_line, _) = byte_to_line_col(source, end_byte);
                let code = source.get(start_byte..end_byte).unwrap_or("").to_string();
                let doc = rust_doc_comment_before(source, start_byte);
                let id = node_id("fn", path, start_line, name);
                funcs.push(RustFunctionDef {
                    id,
                    name: name.to_string(),
                    path: path.to_string(),
                    start_byte,
                    end_byte,
                    start_line,
                    end_line,
                    code,
                    doc,
                });
            }
        }
    } else if matches!(
        kind,
        "struct_item" | "enum_item" | "trait_item" | "type_item" | "impl_item"
    ) {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                let start_byte = node.start_byte();
                let end_byte = node.end_byte();
                let (start_line, _) = byte_to_line_col(source, start_byte);
                let (end_line, _) = byte_to_line_col(source, end_byte);
                let doc = rust_doc_comment_before(source, start_byte);
                let id = node_id("type", path, start_line, name);
                types.push(RustTypeDef {
                    id,
                    name: name.to_string(),
                    path: path.to_string(),
                    start_line,
                    end_line,
                    doc,
                });
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rust_defs(child, source, path, funcs, types);
    }
}

fn collect_rust_calls(
    node: TsNode,
    source: &str,
    path: &str,
    funcs: &[RustFunctionDef],
    calls: &mut Vec<RustCall>,
) {
    match node.kind() {
        // Direct function call: foo(...)
        "call_expression" => {
            if let Some(func_node) = node.child_by_field_name("function") {
                if let Some((callee_name, byte)) = rust_callee_name_and_byte(func_node, source) {
                    if let Some(caller_id) = find_enclosing_function_id(node, funcs) {
                        let snippet = line_snippet_at_byte(source, byte).unwrap_or_default();
                        calls.push(RustCall {
                            caller_id,
                            callee_name,
                            path: path.to_string(),
                            byte,
                            snippet,
                        });
                    }
                }
            }
        }
        // Method call: receiver.method(...)
        "method_call_expression" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                    let byte = name_node.start_byte();
                    if let Some(caller_id) = find_enclosing_function_id(node, funcs) {
                        let snippet = line_snippet_at_byte(source, byte).unwrap_or_default();
                        calls.push(RustCall {
                            caller_id,
                            callee_name: name.to_string(),
                            path: path.to_string(),
                            byte,
                            snippet,
                        });
                    }
                }
            }
        }
        // Function-pointer argument: e.g. iter.map(fn_name) — the identifier is a
        // direct child of the argument list, not itself a call_expression.
        "arguments" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    if let Ok(name) = child.utf8_text(source.as_bytes()) {
                        if funcs.iter().any(|f| f.name == name) {
                            if let Some(caller_id) =
                                find_enclosing_function_id(child, funcs)
                            {
                                let byte = child.start_byte();
                                let snippet =
                                    line_snippet_at_byte(source, byte).unwrap_or_default();
                                calls.push(RustCall {
                                    caller_id,
                                    callee_name: name.to_string(),
                                    path: path.to_string(),
                                    byte,
                                    snippet,
                                });
                            }
                        }
                    }
                }
            }
        }
        // Macro bodies (e.g. format!(...)) are stored as flat token_tree nodes —
        // tree-sitter doesn't parse their contents as structured call_expressions.
        // Text-scan for function name occurrences: both `fn_name(` (call) and
        // bare `fn_name` (function-pointer reference, e.g. .map(fn_name)).
        "token_tree" => {
            if let Ok(raw) = node.utf8_text(source.as_bytes()) {
                if let Some(caller_id) = find_enclosing_function_id(node, funcs) {
                    let start_byte = node.start_byte();
                    let raw_bytes = raw.as_bytes();
                    for func in funcs {
                        if func.id == caller_id {
                            continue; // skip self-references
                        }
                        let name = func.name.as_str();
                        let mut offset = 0usize;
                        while let Some(rel) = raw[offset..].find(name) {
                            let abs = offset + rel;
                            let after = abs + name.len();
                            // Word boundary before the name.
                            let preceded_by_ident = abs > 0 && {
                                let b = raw_bytes[abs - 1];
                                b.is_ascii_alphanumeric() || b == b'_'
                            };
                            // Word boundary after the name.
                            let followed_by_ident = after < raw_bytes.len() && {
                                let b = raw_bytes[after];
                                b.is_ascii_alphanumeric() || b == b'_'
                            };
                            if !preceded_by_ident && !followed_by_ident {
                                let abs_byte = start_byte + abs;
                                let snippet =
                                    line_snippet_at_byte(source, abs_byte).unwrap_or_default();
                                calls.push(RustCall {
                                    caller_id: caller_id.clone(),
                                    callee_name: func.name.clone(),
                                    path: path.to_string(),
                                    byte: abs_byte,
                                    snippet,
                                });
                            }
                            offset += rel + name.len().max(1);
                        }
                    }
                }
            }
            // token_tree children are raw tokens — no further AST recursion needed.
            return;
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rust_calls(child, source, path, funcs, calls);
    }
}

fn find_enclosing_function_id(node: TsNode, funcs: &[RustFunctionDef]) -> Option<String> {
    // Map by byte ranges; linear scan is OK for small repos.
    let start = node.start_byte();
    funcs.iter()
        .find(|f| f.start_byte <= start && start <= f.end_byte)
        .map(|f| f.id.clone())
}

fn rust_callee_name_and_byte(func_node: TsNode, source: &str) -> Option<(String, usize)> {
    // Covers `foo()` and `module::foo()` patterns.
    match func_node.kind() {
        "identifier" => Some((
            func_node.utf8_text(source.as_bytes()).ok()?.to_string(),
            func_node.start_byte(),
        )),
        "scoped_identifier" => {
            // Grab the last identifier in a path.
            let mut cursor = func_node.walk();
            let mut last_ident: Option<TsNode> = None;
            for child in func_node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    last_ident = Some(child);
                }
            }
            let ident = last_ident?;
            Some((
                ident.utf8_text(source.as_bytes()).ok()?.to_string(),
                ident.start_byte(),
            ))
        }
        _ => None,
    }
}

fn build_rust_call_edges(nodes: &[Node], files: &[ScannedFile], calls: &[RustCall]) -> Vec<Edge> {
    let mut name_to_ids: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in nodes.iter().filter(|n| n.kind == NodeKind::Function) {
        name_to_ids.entry(node.label.as_str()).or_default().push(node.id.as_str());
    }

    let mut by_file_source: HashMap<&str, &str> = HashMap::new();
    for file in files {
        by_file_source.insert(file.node.path.as_str(), file.source.as_str());
    }

    let mut edge_map: HashMap<(String, String), Vec<Callsite>> = HashMap::new();

    for call in calls {
        let Some(callee_ids) = name_to_ids.get(call.callee_name.as_str()) else {
            continue;
        };
        // Avoid incorrect edges when multiple defs share a name.
        if callee_ids.len() != 1 {
            continue;
        }
        let callee_id = callee_ids[0].to_string();
        let (line, col) = if let Some(src) = by_file_source.get(call.path.as_str()) {
            byte_to_line_col(src, call.byte)
        } else {
            (1, 1)
        };

        let callsite = Callsite {
            path: call.path.clone(),
            line,
            col,
            snippet: call.snippet.clone(),
            caller: call.caller_id.clone(),
        };
        edge_map
            .entry((call.caller_id.clone(), callee_id))
            .or_default()
            .push(callsite);
    }

    let mut edges = Vec::new();
    for ((source, target), callsites) in edge_map {
        edges.push(Edge {
            source,
            target,
            kind: EdgeKind::Calls,
            label: "calls".to_string(),
            callsites,
        });
    }
    edges
}

fn byte_to_line_col(source: &str, byte: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (idx, ch) in source.char_indices() {
        if idx >= byte {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn line_snippet_at_byte(source: &str, byte: usize) -> Option<String> {
    let mut start = 0usize;
    let mut end = source.len();
    for (idx, ch) in source.char_indices() {
        if idx >= byte {
            end = source[idx..]
                .find('\n')
                .map(|off| idx + off)
                .unwrap_or(source.len());
            break;
        }
        if ch == '\n' {
            start = idx + 1;
        }
    }
    Some(source.get(start..end)?.trim().to_string())
}

fn rust_doc_comment_before(source: &str, start_byte: usize) -> String {
    let prefix = source.get(..start_byte).unwrap_or("");
    let mut lines: Vec<&str> = prefix.lines().collect();
    lines.reverse();
    let mut doc = Vec::new();
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with("///") {
            doc.push(trimmed.trim_start_matches("///").trim());
            continue;
        }
        if trimmed.starts_with("//!") {
            doc.push(trimmed.trim_start_matches("//!").trim());
            continue;
        }
        if trimmed.is_empty() {
            if !doc.is_empty() {
                break;
            }
            continue;
        }
        break;
    }
    doc.reverse();
    doc.join("\n").trim().to_string()
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

fn summarize_file(source: &str) -> String {
    let non_empty = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    format!("{non_empty} non-empty lines scanned.")
}

fn render_html(graph: &Graph, embed_libs: bool) -> String {
    let graph_json = graph_to_json(graph);
    let escaped_title = escape_html(&format!("Codebase map · {}", graph.root));
    let pinned_cdn = [
        "<script src=\"https://unpkg.com/3d-force-graph@1.77.0/dist/3d-force-graph.min.js\"></script>",
        "<script>if(!window.ForceGraph3D){var s=document.createElement('script');s.src='https://cdn.jsdelivr.net/npm/3d-force-graph@1.77.0/dist/3d-force-graph.min.js';document.head.appendChild(s);}</script>",
    ]
    .join("");
    let lib_script = match fs::read_to_string("vendor/3d-force-graph.min.js") {
        Ok(js) => format!(
            "<script>try{{{js}}}catch(e){{window._vendorError=e&&e.message||String(e);}}</script>"
        ),
        Err(_) => {
            if embed_libs {
                eprintln!(
                    "Warning: --embed-libs specified but vendor/3d-force-graph.min.js was not \
                     found; falling back to CDN. Run: curl -L -o vendor/3d-force-graph.min.js \
                     https://unpkg.com/3d-force-graph@1.77.0/dist/3d-force-graph.min.js"
                );
            }
            pinned_cdn
        }
    };

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>{escaped_title}</title>
<script>window._earlyErrors=[];window.addEventListener('error',function(ev){{window._earlyErrors.push(ev.message||String(ev));}});</script>
{lib_script}
<style>
*,*::before,*::after{{box-sizing:border-box;margin:0;padding:0}}
:root{{
  --bg:#07090f;--surface:#0d1117;--surface2:#131920;
  --border:rgba(255,255,255,.07);--border-hi:rgba(99,179,255,.45);
  --text:#e2e8f4;--text-muted:#7a8799;
  --accent-file:#3ddc84;--accent-fn:#4da6ff;--accent-type:#bf7bff;
  --accent-file-dim:rgba(61,220,132,.18);--accent-fn-dim:rgba(77,166,255,.18);--accent-type-dim:rgba(191,123,255,.18);
  --handle:5px;
  font-family:'Inter',ui-sans-serif,system-ui,sans-serif;color-scheme:dark;
}}
html,body{{height:100%;overflow:hidden;background:var(--bg);color:var(--text);font-size:13px;line-height:1.55}}
::-webkit-scrollbar{{width:6px;height:6px}}
::-webkit-scrollbar-track{{background:transparent}}
::-webkit-scrollbar-thumb{{background:rgba(255,255,255,.12);border-radius:3px}}
::-webkit-scrollbar-thumb:hover{{background:rgba(255,255,255,.22)}}
#app{{display:flex;flex-direction:row;height:100vh;overflow:hidden}}
#sidebar-left{{width:300px;min-width:160px;max-width:520px;display:flex;flex-direction:column;background:var(--surface);border-right:1px solid var(--border);flex-shrink:0;overflow:hidden}}
#sidebar-right{{width:340px;min-width:160px;max-width:520px;display:flex;flex-direction:column;background:var(--surface);border-left:1px solid var(--border);flex-shrink:0;overflow:hidden}}
#graph-area{{flex:1 1 0;min-width:0;position:relative;overflow:hidden;background:radial-gradient(ellipse 80% 60% at 50% 40%,#0c1628 0%,var(--bg) 100%)}}
.handle{{width:var(--handle);cursor:col-resize;flex-shrink:0;background:var(--border);transition:background .15s;display:flex;align-items:center;justify-content:center}}
.handle:hover,.handle.dragging{{background:var(--border-hi)}}
.handle::after{{content:'';display:block;width:2px;height:32px;border-radius:2px;background:rgba(255,255,255,.18)}}
.sidebar-header{{padding:18px 16px 12px;border-bottom:1px solid var(--border);flex-shrink:0}}
.sidebar-body{{flex:1 1 0;overflow-y:auto;overflow-x:hidden;padding:14px 16px;display:flex;flex-direction:column;gap:10px}}
.logo{{display:flex;align-items:center;gap:8px;margin-bottom:6px}}
.logo-icon{{width:28px;height:28px;border-radius:8px;background:linear-gradient(135deg,#4da6ff 0%,#bf7bff 100%);display:flex;align-items:center;justify-content:center;font-size:15px;flex-shrink:0}}
.logo-text{{font-size:15px;font-weight:700;letter-spacing:-.3px;background:linear-gradient(90deg,#4da6ff,#bf7bff);-webkit-background-clip:text;-webkit-text-fill-color:transparent}}
.tagline{{color:var(--text-muted);font-size:11.5px;margin-top:2px}}
.stats{{display:grid;grid-template-columns:repeat(4,1fr);gap:6px}}
.stat{{background:var(--surface2);border:1px solid var(--border);border-radius:10px;padding:8px 4px;text-align:center}}
.stat-value{{font-size:17px;font-weight:700;line-height:1}}
.stat-label{{font-size:10px;color:var(--text-muted);text-transform:uppercase;letter-spacing:.06em;margin-top:3px}}
.stat.file .stat-value{{color:var(--accent-file)}}
.stat.fn .stat-value{{color:var(--accent-fn)}}
.stat.type .stat-value{{color:var(--accent-type)}}
.stat.calls .stat-value{{color:#fb923c}}
.search-wrap{{position:relative}}
.search-icon{{position:absolute;left:11px;top:50%;transform:translateY(-50%);opacity:.4;pointer-events:none;font-size:13px}}
input#search{{width:100%;padding:9px 12px 9px 32px;border-radius:10px;border:1px solid var(--border);background:var(--surface2);color:var(--text);font-size:13px;outline:none;transition:border-color .15s}}
input#search:focus{{border-color:var(--border-hi)}}
select{{width:100%;padding:8px 10px;border-radius:10px;border:1px solid var(--border);background:var(--surface2);color:var(--text);font-size:12px;outline:none;cursor:pointer;appearance:none;background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='8' viewBox='0 0 12 8'%3E%3Cpath fill='%237a8799' d='M6 8 0 0h12z'/%3E%3C/svg%3E");background-repeat:no-repeat;background-position:right 10px center}}
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
.pill{{display:inline-block;font-size:10px;font-weight:600;text-transform:uppercase;letter-spacing:.05em;padding:2px 7px;border-radius:999px}}
.pill-file{{background:var(--accent-file-dim);color:var(--accent-file)}}
.pill-function{{background:var(--accent-fn-dim);color:var(--accent-fn)}}
.pill-type{{background:var(--accent-type-dim);color:var(--accent-type)}}
#graph-canvas{{position:absolute;inset:0;cursor:grab}}
#graph-canvas:active{{cursor:grabbing}}
#tooltip{{position:fixed;pointer-events:none;background:rgba(13,17,23,.92);border:1px solid var(--border-hi);border-radius:9px;padding:8px 12px;font-size:12px;color:var(--text);white-space:nowrap;backdrop-filter:blur(8px);display:none;z-index:99;box-shadow:0 4px 24px rgba(0,0,0,.5)}}
.graph-legend{{position:absolute;bottom:18px;left:50%;transform:translateX(-50%);display:flex;gap:14px;background:rgba(13,17,23,.75);border:1px solid var(--border);border-radius:99px;padding:6px 18px;backdrop-filter:blur(8px);pointer-events:none}}
.legend-item{{display:flex;align-items:center;gap:5px;font-size:11px;color:var(--text-muted)}}
.legend-dot{{width:9px;height:9px;border-radius:50%}}
.detail-placeholder{{display:flex;flex-direction:column;align-items:center;justify-content:center;height:100%;text-align:center;padding:32px;gap:10px;opacity:.55}}
.detail-section{{margin-bottom:16px}}
.detail-section h3{{font-size:11px;text-transform:uppercase;letter-spacing:.08em;color:var(--text-muted);margin-bottom:6px;font-weight:600}}
.detail-path{{font-size:11px;color:var(--text-muted);font-family:ui-monospace,monospace}}
.doc-box{{white-space:pre-wrap;background:var(--surface2);border:1px solid var(--border);border-radius:10px;padding:11px 13px;line-height:1.55;font-family:ui-monospace,monospace;font-size:12px;color:#c9d1d9}}
.code-box{{white-space:pre;overflow:auto;background:#010409;border:1px solid var(--border);border-radius:10px;padding:11px 13px;font-family:ui-monospace,monospace;font-size:12px;line-height:1.45;max-height:40vh;color:#c9d1d9}}
.rel-list{{list-style:none;display:flex;flex-direction:column;gap:4px}}
.rel-item{{display:flex;align-items:center;gap:7px;padding:6px 9px;border-radius:8px;background:var(--surface2);border:1px solid var(--border);font-size:12px;cursor:pointer;transition:border-color .12s}}
.rel-item:hover{{border-color:var(--border-hi)}}
.rel-arrow{{color:var(--text-muted);flex-shrink:0}}
.rel-label{{font-size:10px;text-transform:uppercase;letter-spacing:.05em;color:var(--text-muted)}}
.callsite{{padding:6px 9px;border-radius:8px;background:var(--surface2);border:1px solid var(--border);font-size:11px;cursor:pointer;transition:border-color .12s}}
.callsite:hover{{border-color:var(--border-hi)}}
.callsite-loc{{color:var(--text-muted);font-family:ui-monospace,monospace}}
.callsite-snippet{{font-family:ui-monospace,monospace;color:#c9d1d9;margin-top:2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}}
</style>
</head>
<body>
<div id="app">
<aside id="sidebar-left">
  <div class="sidebar-header">
    <div class="logo"><div class="logo-icon">⬡</div><span class="logo-text">Codebase Visualizer</span></div>
    <div class="tagline">Call graph · types · relationships</div>
  </div>
  <div class="sidebar-body">
    <div class="stats" id="stats"></div>
    <select id="viewMode">
      <option value="callgraph">Call graph</option>
      <option value="types">Data structures</option>
    </select>
    <div class="search-wrap">
      <span class="search-icon">⌕</span>
      <input id="search" placeholder="Search by name, path, docs…" autocomplete="off" spellcheck="false"/>
    </div>
    <div class="results-count" id="results-count"></div>
    <div class="item-list" id="results"></div>
  </div>
</aside>
<div class="handle" id="handle-left" title="Drag to resize"></div>
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
</div>
<script>
const GRAPH = {graph_json};
const COLORS = {{ file:'#3ddc84', function:'#4da6ff', type:'#bf7bff' }};
const RADII  = {{ file:11, function:8, type:9 }};
const S = {{
  selected:null, visible:[], sim:[], edgesVis:[],
  draggingNode:null, panning:false, lastMx:0, lastMy:0, hot:null, dirty:true,
}};
const searchEl   = document.getElementById('search');
const viewModeEl = document.getElementById('viewMode');
const resultsCt  = document.getElementById('results');
const countEl    = document.getElementById('results-count');
const detailsCt  = document.getElementById('details-body');
const canvas     = document.getElementById('graph-canvas');
const tip        = document.getElementById('tooltip');
const ctx        = canvas.getContext('2d');
const params     = new URLSearchParams(window.location.search);
const reqSelect  = params.get('select');
if (params.get('q')) searchEl.value = params.get('q');
if (['callgraph','types'].includes(params.get('view'))) viewModeEl.value = params.get('view');

document.getElementById('stats').innerHTML = [
  ['file','file',GRAPH.stats.files||0,'Files'],
  ['fn','function',GRAPH.stats.functions||0,'Funcs'],
  ['type','type',GRAPH.stats.types||0,'Types'],
  ['calls','calls',GRAPH.stats.calls||0,'Calls'],
].map(([cls,_k,v,l])=>`<div class="stat ${{cls}}"><div class="stat-value">${{v}}</div><div class="stat-label">${{l}}</div></div>`).join('');

function buildAdjacency(links) {{
  const out=new Map(), inc=new Map();
  for (const l of links) {{
    if (!out.has(l.source)) out.set(l.source,new Set());
    if (!inc.has(l.target)) inc.set(l.target,new Set());
    out.get(l.source).add(l.target); inc.get(l.target).add(l.source);
  }}
  return {{out,inc}};
}}
function dataset() {{
  const mode=viewModeEl.value;
  if (mode==='callgraph') {{
    return {{nodes:GRAPH.nodes.filter(n=>n.kind==='function'),links:GRAPH.edges.filter(e=>e.kind==='calls')}};
  }}
  const typeIds=new Set(GRAPH.nodes.filter(n=>n.kind==='type').map(n=>n.id));
  return {{
    nodes:GRAPH.nodes.filter(n=>n.kind==='file'||n.kind==='type'),
    links:GRAPH.edges.filter(e=>e.kind==='contains'&&typeIds.has(e.target)),
  }};
}}
function filteredDataset() {{
  const q=searchEl.value.trim().toLowerCase();
  const base=dataset();
  if (!q) return base;
  const {{out,inc}}=buildAdjacency(base.links);
  const keep=new Set();
  for (const n of base.nodes) {{
    if ((n.label+' '+n.path+' '+n.doc+' '+n.summary).toLowerCase().includes(q)) {{
      keep.add(n.id);
      for (const nb of (out.get(n.id)||[])) keep.add(nb);
      for (const nb of (inc.get(n.id)||[])) keep.add(nb);
    }}
  }}
  const nodes=base.nodes.filter(n=>keep.has(n.id));
  const keepIds=new Set(nodes.map(n=>n.id));
  return {{nodes, links:base.links.filter(l=>keepIds.has(l.source)&&keepIds.has(l.target))}};
}}

const K_SPRING=0.018,K_REPEL=7000,K_DAMP=0.82,REST_LEN=140,CENTER_K=0.012;
function initSim() {{
  const W=canvas.width,H=canvas.height,cx=W/2,cy=H/2;
  const byId=new Map(S.sim.map(n=>[n.id,n]));
  const ds=filteredDataset();
  S.visible=ds.nodes; S.edgesVis=ds.links;
  S.sim=S.visible.map((node,i)=>{{
    const old=byId.get(node.id);
    if (old) return {{...old,node}};
    const angle=Math.PI*2*i/Math.max(S.visible.length,1);
    const r=180+Math.random()*120;
    return {{id:node.id,node,x:cx+Math.cos(angle)*r,y:cy+Math.sin(angle)*r,vx:0,vy:0}};
  }});
  S.dirty=true;
}}
function tick() {{
  if (S.draggingNode) return;
  const cx=canvas.width/2,cy=canvas.height/2; let moving=false;
  for (let i=0;i<S.sim.length;i++) {{
    const a=S.sim[i];
    for (let j=i+1;j<S.sim.length;j++) {{
      const b=S.sim[j],dx=a.x-b.x,dy=a.y-b.y,d2=dx*dx+dy*dy+1,f=K_REPEL/d2;
      a.vx+=f*dx;a.vy+=f*dy;b.vx-=f*dx;b.vy-=f*dy;
    }}
  }}
  const pm=new Map(S.sim.map(n=>[n.id,n]));
  for (const e of S.edgesVis) {{
    const a=pm.get(e.source),b=pm.get(e.target);
    if (!a||!b) continue;
    const dx=b.x-a.x,dy=b.y-a.y,dist=Math.sqrt(dx*dx+dy*dy)+0.01,f=K_SPRING*(dist-REST_LEN);
    const fx=f*dx/dist,fy=f*dy/dist;
    a.vx+=fx;a.vy+=fy;b.vx-=fx;b.vy-=fy;
  }}
  for (const n of S.sim) {{
    n.vx+=(cx-n.x)*CENTER_K;n.vy+=(cy-n.y)*CENTER_K;
    n.vx*=K_DAMP;n.vy*=K_DAMP;n.x+=n.vx;n.y+=n.vy;
    if (Math.abs(n.vx)>0.05||Math.abs(n.vy)>0.05) moving=true;
  }}
  if (moving) S.dirty=true;
}}
function draw() {{
  const W=canvas.width,H=canvas.height;
  ctx.clearRect(0,0,W,H);
  ctx.save();ctx.strokeStyle='rgba(255,255,255,.028)';ctx.lineWidth=1;
  for (let x=0;x<W;x+=50){{ctx.beginPath();ctx.moveTo(x,0);ctx.lineTo(x,H);ctx.stroke();}}
  for (let y=0;y<H;y+=50){{ctx.beginPath();ctx.moveTo(0,y);ctx.lineTo(W,y);ctx.stroke();}}
  ctx.restore();
  const pm=new Map(S.sim.map(n=>[n.id,n]));
  for (const e of S.edgesVis) {{
    const a=pm.get(e.source),b=pm.get(e.target);
    if (!a||!b) continue;
    const rel=S.selected&&(e.source===S.selected||e.target===S.selected);
    ctx.beginPath();ctx.moveTo(a.x,a.y);ctx.lineTo(b.x,b.y);
    ctx.strokeStyle=rel?'rgba(99,179,255,.55)':'rgba(255,255,255,.07)';
    ctx.lineWidth=rel?1.8:1;ctx.stroke();
  }}
  for (const n of S.sim) {{
    const r=RADII[n.node.kind]||8,col=COLORS[n.node.kind]||'#aaa';
    const isSel=n.id===S.selected,isHot=n.id===S.hot;
    const isRel=S.selected&&S.edgesVis.some(e=>(e.source===S.selected&&e.target===n.id)||(e.target===S.selected&&e.source===n.id));
    ctx.save();
    if (isSel||isHot){{ctx.shadowColor=col;ctx.shadowBlur=isSel?28:14;}}
    else if (isRel){{ctx.shadowColor=col;ctx.shadowBlur=10;}}
    if (isSel){{
      ctx.beginPath();ctx.arc(n.x,n.y,r+5,0,Math.PI*2);
      ctx.strokeStyle=col;ctx.globalAlpha=0.45;ctx.lineWidth=2;ctx.stroke();ctx.globalAlpha=1;
    }}
    ctx.beginPath();ctx.arc(n.x,n.y,r,0,Math.PI*2);
    if (isSel||isHot){{ctx.fillStyle=col;}}
    else if (isRel){{ctx.fillStyle=col;ctx.globalAlpha=0.75;}}
    else{{
      ctx.globalAlpha=S.selected?0.35:1;
      const g=ctx.createRadialGradient(n.x-r*.25,n.y-r*.25,r*.1,n.x,n.y,r);
      g.addColorStop(0,col);g.addColorStop(1,col+'88');ctx.fillStyle=g;
    }}
    ctx.fill();ctx.globalAlpha=1;ctx.shadowBlur=0;
    ctx.font=`${{(isSel||isHot)?'600 ':'400 '}}11px Inter,sans-serif`;
    ctx.fillStyle=(S.selected&&!isSel&&!isRel)?'rgba(226,232,244,.3)':'#e2e8f4';
    ctx.textBaseline='middle';ctx.fillText(n.node.label,n.x+r+5,n.y);
    ctx.restore();
  }}
}}
function loop(){{tick();if(S.dirty){{draw();S.dirty=false;}}requestAnimationFrame(loop);}}
function resizeCanvas(){{
  const a=document.getElementById('graph-area');
  canvas.width=a.clientWidth;canvas.height=a.clientHeight;S.dirty=true;
}}
function hitTest(mx,my){{
  for (const n of S.sim){{const r=RADII[n.node.kind]||8,dx=n.x-mx,dy=n.y-my;if(dx*dx+dy*dy<=(r+4)*(r+4))return n;}}return null;
}}
canvas.addEventListener('mousedown',e=>{{
  const n=hitTest(e.offsetX,e.offsetY);
  if(n){{S.draggingNode=n;selectNode(n.id);}}
  else{{S.panning=true;S.lastMx=e.clientX;S.lastMy=e.clientY;}}
}});
canvas.addEventListener('mousemove',e=>{{
  const n=hitTest(e.offsetX,e.offsetY);
  if(n!==null?n.id:null!==S.hot){{S.hot=n?n.id:null;S.dirty=true;}}
  if(S.draggingNode){{S.draggingNode.x=e.offsetX;S.draggingNode.y=e.offsetY;S.draggingNode.vx=0;S.draggingNode.vy=0;S.dirty=true;}}
  else if(S.panning){{for(const nd of S.sim){{nd.x+=e.clientX-S.lastMx;nd.y+=e.clientY-S.lastMy;}}S.lastMx=e.clientX;S.lastMy=e.clientY;S.dirty=true;}}
  if(n){{tip.style.display='block';tip.style.left=(e.clientX+14)+'px';tip.style.top=(e.clientY-8)+'px';tip.textContent=`${{n.node.kind.toUpperCase()}}  ${{n.node.label}}  ·  ${{n.node.path}}:${{n.node.line}}`;}}
  else{{tip.style.display='none';}}
}});
window.addEventListener('mouseup',()=>{{S.draggingNode=null;S.panning=false;}});
canvas.addEventListener('wheel',e=>{{
  e.preventDefault();const f=e.deltaY<0?1.1:0.91,cx=e.offsetX,cy=e.offsetY;
  for(const n of S.sim){{n.x=cx+(n.x-cx)*f;n.y=cy+(n.y-cy)*f;}}S.dirty=true;
}},{{passive:false}});

function selectNode(id){{S.selected=id;S.dirty=true;renderDetails();renderResults();}}

function pillClass(k){{return k==='file'?'pill-file':k==='function'?'pill-function':'pill-type';}}
function activeClass(n){{
  if(S.selected!==n.id)return'';
  return n.kind==='file'?'active-file':n.kind==='type'?'active-type':'active';
}}
function renderResults(){{
  const ds=filteredDataset(),vis=ds.nodes.slice(0,100),tot=ds.nodes.length;
  countEl.textContent=tot===0?'No matches':`${{tot}} node${{tot===1?'':'s'}}${{tot>100?' (showing 100)':''}}`;
  resultsCt.innerHTML=vis.map(node=>`
    <button class="item ${{activeClass(node)}}" data-id="${{node.id}}">
      <div class="item-top"><span class="pill ${{pillClass(node.kind)}}">${{node.kind}}</span><span class="item-name">${{escapeHtml(node.label)}}</span></div>
      <div class="item-path">${{escapeHtml(node.path)}}:${{node.line}}</div>
      <div class="item-summary">${{escapeHtml(node.summary)}}</div>
    </button>`).join('');
  resultsCt.querySelectorAll('button').forEach(b=>b.addEventListener('click',()=>selectNode(b.dataset.id)));
}}
function renderDetails(){{
  const id=S.selected,node=GRAPH.nodes.find(n=>n.id===id);
  if(!node){{
    detailsCt.innerHTML=`<div class="detail-placeholder">
      <svg width="48" height="48" viewBox="0 0 48 48" fill="none"><circle cx="24" cy="24" r="20" stroke="currentColor" stroke-width="2"/><circle cx="24" cy="24" r="7" stroke="currentColor" stroke-width="2"/><line x1="24" y1="4" x2="24" y2="17" stroke="currentColor" stroke-width="2"/><line x1="24" y1="31" x2="24" y2="44" stroke="currentColor" stroke-width="2"/><line x1="4" y1="24" x2="17" y2="24" stroke="currentColor" stroke-width="2"/><line x1="31" y1="24" x2="44" y2="24" stroke="currentColor" stroke-width="2"/></svg>
      <div style="font-weight:600;color:var(--text)">Select a node</div>
      <div style="font-size:12px">Click any node in the graph<br>or pick one from the list</div></div>`;
    return;
  }}
  const ac=COLORS[node.kind]||'#aaa';
  const outE=GRAPH.edges.filter(e=>e.source===id&&e.kind==='calls');
  const inE=GRAPH.edges.filter(e=>e.target===id&&e.kind==='calls');
  const relE=GRAPH.edges.filter(e=>(e.source===id||e.target===id)&&e.kind!=='calls').slice(0,12);
  const callsites=inE.flatMap(e=>(e.callsites||[]).map(s=>Object.assign({{}},s,{{callerId:e.source}}))).slice(0,80);
  detailsCt.innerHTML=`
    <div style="padding-bottom:14px;margin-bottom:14px;border-bottom:1px solid var(--border)">
      <div style="display:flex;align-items:center;gap:8px;margin-bottom:6px">
        <span style="width:10px;height:10px;border-radius:50%;background:${{ac}};flex-shrink:0;box-shadow:0 0 8px ${{ac}}"></span>
        <span class="pill ${{pillClass(node.kind)}}">${{node.kind}}</span>
        <span style="font-size:11px;color:var(--text-muted)">${{escapeHtml(node.language)}}</span>
      </div>
      <h2 style="font-size:16px;font-weight:700;word-break:break-all">${{escapeHtml(node.label)}}</h2>
      <div class="detail-path">${{escapeHtml(node.path)}}:${{node.line}}${{node.endLine&&node.endLine!==node.line?'–'+node.endLine:''}}</div>
    </div>
    ${{node.doc?`<div class="detail-section"><h3>Documentation</h3><div class="doc-box">${{escapeHtml(node.doc)}}</div></div>`:''}}
    ${{node.code?`<div class="detail-section"><h3>Code</h3><pre class="code-box">${{escapeHtml(node.code)}}</pre></div>`:''}}
    ${{callsites.length?`<div class="detail-section"><h3>Called by (${{callsites.length}})</h3><div style="display:flex;flex-direction:column;gap:4px">
      ${{callsites.map(s=>`<div class="callsite" data-caller="${{escapeHtml(s.callerId)}}">
        <div><strong>${{escapeHtml(labelFor(s.callerId))}}</strong> <span class="callsite-loc">${{escapeHtml(s.path)}}:${{s.line}}:${{s.col}}</span></div>
        ${{s.snippet?`<div class="callsite-snippet">${{escapeHtml(s.snippet)}}</div>`:''}}
      </div>`).join('')}}</div></div>`:''}}
    ${{outE.length?`<div class="detail-section"><h3>Calls (${{outE.length}})</h3><ul class="rel-list">
      ${{outE.slice(0,30).map(e=>`<li class="rel-item" data-id="${{escapeHtml(e.target)}}">
        <span class="rel-arrow">→</span><span style="font-size:12px">${{escapeHtml(labelFor(e.target))}}</span>
        <span style="margin-left:auto"><span class="pill pill-function">fn</span></span></li>`).join('')}}
    </ul></div>`:''}}
    ${{relE.length?`<div class="detail-section"><h3>Relationships (${{relE.length}})</h3><ul class="rel-list">
      ${{relE.map(e=>{{const o=e.source===id?e.target:e.source;const on=GRAPH.nodes.find(n=>n.id===o);const ar=e.source===id?'→':'←';
        return`<li class="rel-item" data-id="${{escapeHtml(o)}}"><span class="rel-label">${{escapeHtml(e.label)}}</span><span class="rel-arrow">${{ar}}</span><span style="font-size:12px">${{escapeHtml(on?.label||o)}}</span>
        ${{on?`<span style="margin-left:auto"><span class="pill ${{pillClass(on.kind)}}">${{on.kind}}</span></span>`:''}}
        </li>`;
      }}).join('')}}</ul></div>`:''}}
  `;
  detailsCt.querySelectorAll('.callsite[data-caller]').forEach(el=>el.addEventListener('click',()=>selectNode(el.dataset.caller)));
  detailsCt.querySelectorAll('.rel-item[data-id]').forEach(el=>el.addEventListener('click',()=>selectNode(el.dataset.id)));
  detailsCt.scrollTop=0;
}}
function labelFor(id){{return GRAPH.nodes.find(n=>n.id===id)?.label||id;}}
function fullRender(){{
  if(!S.selected&&reqSelect){{
    const req=reqSelect.toLowerCase();
    const hit=GRAPH.nodes.find(n=>n.label.toLowerCase()===req)||GRAPH.nodes.find(n=>n.id.toLowerCase().includes(req));
    if(hit)S.selected=hit.id;
  }}
  initSim();renderResults();renderDetails();
}}
function makeHandle(h,s,side){{
  let dr=false,sx=0,sw=0;
  h.addEventListener('mousedown',e=>{{dr=true;sx=e.clientX;sw=s.offsetWidth;h.classList.add('dragging');document.body.style.userSelect='none';document.body.style.cursor='col-resize';}});
  window.addEventListener('mousemove',e=>{{if(!dr)return;const d=side==='left'?e.clientX-sx:sx-e.clientX;s.style.width=Math.max(160,Math.min(520,sw+d))+'px';resizeCanvas();S.dirty=true;}});
  window.addEventListener('mouseup',()=>{{if(!dr)return;dr=false;h.classList.remove('dragging');document.body.style.userSelect='';document.body.style.cursor='';}});
}}
makeHandle(document.getElementById('handle-left'),  document.getElementById('sidebar-left'),  'left');
makeHandle(document.getElementById('handle-right'), document.getElementById('sidebar-right'), 'right');
function escapeHtml(v){{return String(v).replace(/[&<>"']/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#039;'}}[c]));}}
const ro=new ResizeObserver(()=>{{resizeCanvas();S.dirty=true;}});
ro.observe(document.getElementById('graph-area'));
resizeCanvas();
searchEl.addEventListener('input',fullRender);
viewModeEl.addEventListener('change',()=>{{S.selected=null;fullRender();}});
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
        "{{\"id\":{},\"label\":{},\"kind\":{},\"path\":{},\"line\":{},\"endLine\":{},\"language\":{},\"doc\":{},\"summary\":{},\"code\":{}}}",
        json_string(&node.id),
        json_string(&node.label),
        json_string(kind_label(&node.kind)),
        json_string(&node.path),
        node.line,
        node.end_line,
        json_string(&node.language),
        json_string(&node.doc),
        json_string(&node.summary),
        json_string(&node.code)
    )
}

fn edge_to_json(edge: &Edge) -> String {
    format!(
        "{{\"source\":{},\"target\":{},\"kind\":{},\"label\":{},\"callsites\":[{}]}}",
        json_string(&edge.source),
        json_string(&edge.target),
        json_string(edge_kind_label(&edge.kind)),
        json_string(&edge.label),
        edge.callsites
            .iter()
            .map(callsite_to_json)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn edge_kind_label(kind: &EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Contains => "contains",
        EdgeKind::Calls => "calls",
    }
}

fn callsite_to_json(site: &Callsite) -> String {
    format!(
        "{{\"path\":{},\"line\":{},\"col\":{},\"snippet\":{},\"caller\":{}}}",
        json_string(&site.path),
        site.line,
        site.col,
        json_string(&site.snippet),
        json_string(&site.caller)
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Escape `</` as `\u003c/` so that `</script>` or `</style>` inside a
        // JSON string embedded in an HTML <script> block never triggers the
        // browser's HTML tokeniser and closes the script prematurely.
        if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            out.push_str("\\u003c");
            i += 1;
            continue;
        }
        // Also escape `<!--` to prevent HTML comment injection.
        if bytes[i] == b'<' && bytes[i..].starts_with(b"<!--") {
            out.push_str("\\u003c");
            i += 1;
            continue;
        }
        let ch = value[i..].chars().next().unwrap();
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
        i += ch.len_utf8();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
                end_line: 1,
                language: "rust".to_string(),
                doc: "Runs the app.".to_string(),
                summary: "Runs the app".to_string(),
                code: "fn run() {}".to_string(),
            }],
            edges: Vec::new(),
            stats: BTreeMap::from([("functions".to_string(), 1)]),
        };

        let html = render_html(&graph, false);

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
        // Rust-only callgraph: we still count file nodes, but only Rust functions/types are indexed.
        assert_eq!(graph.stats.get("functions"), Some(&2));
        assert_eq!(graph.stats.get("types"), Some(&0));
        assert!(graph.edges.iter().any(|edge| edge.label == "calls"));
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
