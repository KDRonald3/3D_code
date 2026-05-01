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
    let escaped_title = escape_html(&format!("Codebase map: {}", graph.root));
    let pinned_cdn = [
        "<script src=\"https://unpkg.com/3d-force-graph@1.77.0/dist/3d-force-graph.min.js\"></script>",
        "<script>if(!window.ForceGraph3D){var s=document.createElement('script');s.src='https://cdn.jsdelivr.net/npm/3d-force-graph@1.77.0/dist/3d-force-graph.min.js';document.head.appendChild(s);}</script>",
    ]
    .join("");
    // Always try to embed the local vendor bundle first so the HTML works offline.
    // Fall back to CDN only when the file is absent.
    // Always try to embed the local vendor bundle first so the HTML works offline.
    // Wrap in try/catch so any runtime error during bundle init is captured.
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
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>{escaped_title}</title>
<script>window._earlyErrors=[];window.addEventListener('error',function(ev){{window._earlyErrors.push(ev.message||String(ev));}});</script>
{lib_script}
<style>
:root {{ color-scheme: dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; }}
body {{ margin: 0; background: #0d1117; color: #e6edf3; }}
.app {{ display: grid; grid-template-columns: 360px 1fr 420px; min-height: 100vh; }}
aside, main {{ border-right: 1px solid #30363d; }}
aside, .details {{ padding: 18px; overflow: auto; }}
h1 {{ font-size: 22px; margin: 0 0 8px; }}
.muted {{ color: #8b949e; font-size: 13px; }}
.stats {{ display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; margin: 18px 0; }}
.stat {{ background: #161b22; border: 1px solid #30363d; border-radius: 12px; padding: 10px; }}
.stat strong {{ display: block; font-size: 24px; }}
input, select {{ width: 100%; box-sizing: border-box; margin: 8px 0; padding: 10px 12px; border-radius: 10px; border: 1px solid #30363d; color: #e6edf3; background: #010409; }}
.list {{ display: grid; gap: 8px; margin-top: 12px; }}
button.item {{ text-align: left; border: 1px solid #30363d; background: #161b22; color: #e6edf3; padding: 10px; border-radius: 10px; cursor: pointer; }}
button.item:hover, button.item.active {{ border-color: #58a6ff; background: #0b2442; }}
.pill {{ display: inline-block; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; padding: 2px 7px; border-radius: 999px; background: #21262d; color: #a5d6ff; }}
main {{ position: relative; overflow: hidden; }}
#graph3d {{ width: 100%; height: 100vh; background: radial-gradient(circle at top left, #172033, #0d1117 42%); }}
.details h2 {{ margin-top: 0; }}
.doc {{ white-space: pre-wrap; background: #161b22; border: 1px solid #30363d; border-radius: 12px; padding: 12px; line-height: 1.45; }}
.code {{ white-space: pre; overflow: auto; background: #010409; border: 1px solid #30363d; border-radius: 12px; padding: 12px; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace; font-size: 12px; line-height: 1.45; max-height: 46vh; }}
.usedBy {{ display: grid; gap: 8px; }}
.usedByItem {{ background: #161b22; border: 1px solid #30363d; border-radius: 12px; padding: 10px; cursor: pointer; }}
.usedByItem:hover {{ border-color: #58a6ff; }}
.errorBox {{ border: 1px solid #f85149; background: #2d0b0b; border-radius: 12px; padding: 12px; white-space: pre-wrap; }}
</style>
</head>
<body>
<div class="app">
<aside>
<h1>Codebase Visualizer</h1>
<div class="muted">3D Rust call graph + code sidebar. Click a function to follow callers/callees and inspect code.</div>
<div class="stats" id="stats"></div>
<select id="viewMode">
  <option value="callgraph">Call graph</option>
  <option value="types">Data structures</option>
</select>
<input id="search" placeholder="Search label, path, docs..." autofocus />
<div class="list" id="results"></div>
</aside>
<main><div id="graph3d" role="img" aria-label="3D codebase graph"></div></main>
<section class="details" id="details">
<h2>Select a node</h2>
<p class="muted">Click a function node to see its code and where it’s used.</p>
</section>
</div>
<script>
const graph = {graph_json};
const state = {{ selected: null }};
const colors = {{ file: '#7ee787', function: '#58a6ff', type: '#d2a8ff' }};
const search = document.getElementById('search');
const viewMode = document.getElementById('viewMode');
const results = document.getElementById('results');
const details = document.getElementById('details');
const graphEl = document.getElementById('graph3d');
const params = new URLSearchParams(window.location.search);
const requestedSelect = params.get('select');

if (params.get('q')) search.value = params.get('q');
if (['callgraph', 'types'].includes(params.get('view'))) viewMode.value = params.get('view');

document.getElementById('stats').innerHTML = [
  ['Files', graph.stats.files || 0],
  ['Funcs', graph.stats.functions || 0],
  ['Types', graph.stats.types || 0],
  ['Calls', graph.stats.calls || 0],
].map(([label, value]) => `<div class="stat"><strong>${{value}}</strong><span>${{label}}</span></div>`).join('');

function escapeHtml(value) {{
  return String(value).replace(/[&<>"']/g, ch => ({{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#039;'}}[ch]));
}}

function labelFor(id) {{
  return graph.nodes.find(node => node.id === id)?.label || id;
}}

function buildAdjacency(links) {{
  const out = new Map();
  const inc = new Map();
  for (const link of links) {{
    if (!out.has(link.source)) out.set(link.source, new Set());
    if (!inc.has(link.target)) inc.set(link.target, new Set());
    out.get(link.source).add(link.target);
    inc.get(link.target).add(link.source);
  }}
  return {{ out, inc }};
}}

function dataset() {{
  const mode = viewMode.value;
  if (mode === 'callgraph') {{
    const nodes = graph.nodes.filter(n => n.kind === 'function');
    const links = graph.edges.filter(e => e.kind === 'calls');
    return {{ nodes, links }};
  }}
  const typeIds = new Set(graph.nodes.filter(n => n.kind === 'type').map(n => n.id));
  const nodes = graph.nodes.filter(n => n.kind === 'file' || n.kind === 'type');
  const links = graph.edges.filter(e => e.kind === 'contains' && typeIds.has(e.target));
  return {{ nodes, links }};
}}

function matchesNode(node, q) {{
  if (!q) return true;
  const haystack = `${{node.label}} ${{node.path}} ${{node.doc}} ${{node.summary}}`.toLowerCase();
  return haystack.includes(q);
}}

function filteredDataset() {{
  const q = search.value.trim().toLowerCase();
  const base = dataset();
  if (!q) return base;

  const {{ out, inc }} = buildAdjacency(base.links);
  const keep = new Set();
  for (const n of base.nodes) {{
    if (matchesNode(n, q)) {{
      keep.add(n.id);
      for (const nb of (out.get(n.id) || [])) keep.add(nb);
      for (const nb of (inc.get(n.id) || [])) keep.add(nb);
    }}
  }}
  const nodes = base.nodes.filter(n => keep.has(n.id));
  const keepIds = new Set(nodes.map(n => n.id));
  const links = base.links.filter(l => keepIds.has(l.source) && keepIds.has(l.target));
  return {{ nodes, links }};
}}

function showInitError(message) {{
  graphEl.innerHTML =
    '<div class="errorBox"><strong>3D graph failed to initialize</strong>' +
    '<br><br>' + escapeHtml(message) +
    '<br><br>Try:<br>' +
    '&bull; Hard-refresh (Ctrl+Shift+R)<br>' +
    '&bull; Open DevTools Console for the full error<br>' +
    '&bull; Re-generate with: <code>cargo run -- . --output codebase-map.html</code>' +
    '</div>';
}}

function idOf(value) {{
  return (value && typeof value === 'object') ? value.id : value;
}}

// _earlyErrors was started in <head> before the vendor bundle; reuse it here.
const _scriptErrors = window._earlyErrors || [];

let fg = null;
function initForceGraph() {{
  // Accept either the global name or window property (handles some strict-mode edge-cases).
  const FG3D = window.ForceGraph3D || (typeof ForceGraph3D !== 'undefined' ? ForceGraph3D : null);
  if (typeof FG3D !== 'function') return false;
  // Set explicit pixel dimensions so the WebGL renderer gets a non-zero canvas size.
  const w = graphEl.clientWidth || graphEl.parentElement?.clientWidth || window.innerWidth;
  const h = (graphEl.clientHeight || window.innerHeight);
  graphEl.style.width = w + 'px';
  graphEl.style.height = h + 'px';
  try {{
    fg = FG3D()(graphEl)
      .backgroundColor('#0d1117')
      .nodeId('id')
      .nodeLabel(node => `${{node.label}}\\n${{node.path}}:${{node.line}}`)
      .nodeRelSize(6)
      .linkSource('source')
      .linkTarget('target')
      .onNodeClick(node => selectNode(node.id));
  }} catch (err) {{
    _scriptErrors.push('ForceGraph3D() threw: ' + err.message);
    return false;
  }}
  return true;
}}

function renderResults() {{
  const ds = filteredDataset();
  results.innerHTML = ds.nodes.slice(0, 120).map(node => `
    <button class="item ${{state.selected === node.id ? 'active' : ''}}" data-id="${{escapeHtml(node.id)}}">
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
  renderDetails();
  renderResults();
  renderGraph();
}}

function renderDetails() {{
  const id = state.selected;
  const node = graph.nodes.find(candidate => candidate.id === id);
  if (!node) {{
    details.innerHTML = `
      <h2>Select a node</h2>
      <p class="muted">Click a function node to see its code and where it’s used.</p>
    `;
    details.scrollTop = 0;
    return;
  }}

  const incoming = graph.edges.filter(e => e.kind === 'calls' && e.target === id);
  const outgoing = graph.edges.filter(e => e.kind === 'calls' && e.source === id);
  const usedBy = incoming.flatMap(e => (e.callsites || []).map(site => ({{ ...site, callerId: e.source }})));

  details.innerHTML = `
    <h2>${{escapeHtml(node.label)}}</h2>
    <p><span class="pill">${{node.kind}}</span> <span class="muted">${{escapeHtml(node.language)}} - ${{escapeHtml(node.path)}}:${{node.line}}-${{node.endLine || node.line}}</span></p>
    <h3>Documentation</h3><div class="doc">${{escapeHtml(node.doc || 'No nearby documentation comment found.')}}</div>
    <h3>Code</h3><pre class="code">${{escapeHtml(node.code || 'No code captured for this node.')}}</pre>
    <h3>Used by (${{usedBy.length}})</h3>
    <div class="usedBy">
      ${{usedBy.slice(0, 250).map(site => `
        <div class="usedByItem" data-caller="${{escapeHtml(site.callerId)}}" title="Click to jump to caller">
          <div><strong>${{escapeHtml(labelFor(site.callerId))}}</strong> <span class="muted">${{escapeHtml(site.path)}}:${{site.line}}:${{site.col}}</span></div>
          <div class="muted">${{escapeHtml(site.snippet || '')}}</div>
        </div>
      `).join('') || '<div class="muted">No recorded call sites.</div>'}}
    </div>
    <h3>Calls (${{outgoing.length}})</h3>
    <div class="usedBy">
      ${{outgoing.slice(0, 150).map(e => `
        <div class="usedByItem" data-target="${{escapeHtml(e.target)}}" title="Click to jump to callee">
          <div><strong>${{escapeHtml(labelFor(e.target))}}</strong></div>
        </div>
      `).join('') || '<div class="muted">No outgoing calls recorded.</div>'}}
    </div>
  `;
  details.scrollTop = 0;

  details.querySelectorAll('.usedByItem[data-caller]').forEach(el => {{
    el.addEventListener('click', () => {{
      const caller = el.getAttribute('data-caller');
      if (caller) selectNode(caller);
    }});
  }});
  details.querySelectorAll('.usedByItem[data-target]').forEach(el => {{
    el.addEventListener('click', () => {{
      const target = el.getAttribute('data-target');
      if (target) selectNode(target);
    }});
  }});
}}

function renderGraph() {{
  if (!fg) return;
  const ds = filteredDataset();
  const selected = state.selected;
  const {{ out, inc }} = buildAdjacency(ds.links);
  const neighbors = new Set();
  if (selected) {{
    neighbors.add(selected);
    for (const t of (out.get(selected) || [])) neighbors.add(t);
    for (const s of (inc.get(selected) || [])) neighbors.add(s);
  }}

  fg.graphData({{ nodes: ds.nodes, links: ds.links }});
  fg.nodeColor(node => {{
    if (!selected) return colors[node.kind] || '#58a6ff';
    if (node.id === selected) return '#f0f6fc';
    if (neighbors.has(node.id)) return colors[node.kind] || '#58a6ff';
    return '#30363d';
  }});
  fg.linkColor(link => {{
    const s = idOf(link.source);
    const t = idOf(link.target);
    if (!selected) return '#41546f';
    if (s === selected || t === selected) return '#58a6ff';
    return '#30363d';
  }});
  fg.linkWidth(link => {{
    const s = idOf(link.source);
    const t = idOf(link.target);
    return (selected && (s === selected || t === selected)) ? 2.5 : 1;
  }});

  // Delay zoomToFit so the force simulation has time to stabilize.
  setTimeout(() => {{
    if (!fg) return;
    if (selected) {{
      fg.zoomToFit(600, 70, n => n.id === selected || neighbors.has(n.id));
    }} else {{
      fg.zoomToFit(600, 70);
    }}
  }}, 500);
}}

function renderAll() {{
  if (!state.selected && requestedSelect) {{
    const requested = requestedSelect.toLowerCase();
    const match = graph.nodes.find(n => n.label.toLowerCase() === requested) || graph.nodes.find(n => n.id.toLowerCase().includes(requested));
    if (match) state.selected = match.id;
  }}
  renderResults();
  renderDetails();
  if (!fg) {{
    if (!initForceGraph()) {{
      setTimeout(() => {{
        if (!fg && !initForceGraph()) {{
          const parts = ['ForceGraph3D is not available after waiting.'];
          if (window._vendorError) parts.push('Vendor bundle error: ' + window._vendorError);
          const allErrs = (window._earlyErrors || []).concat(_scriptErrors);
          if (allErrs.length) parts.push('JS errors: ' + allErrs.join(' | '));
          showInitError(parts.join(' '));
        }} else {{
          renderGraph();
        }}
      }}, 250);
      return;
    }}
  }}
  renderGraph();
}}

search.addEventListener('input', renderAll);
viewMode.addEventListener('change', () => {{ state.selected = null; renderAll(); }});
window.addEventListener('resize', () => {{
  if (!fg) return;
  const w = graphEl.parentElement ? graphEl.parentElement.clientWidth : window.innerWidth;
  const h = window.innerHeight;
  graphEl.style.width = w + 'px';
  graphEl.style.height = h + 'px';
  fg.width(w).height(h);
}});
renderAll();
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
        assert!(html.contains("3D Rust call graph"));
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
