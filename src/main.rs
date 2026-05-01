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
            summary: format!(
                "fn {} (lines {}-{})",
                def.name, def.start_line, def.end_line
            ),
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
            summary: format!(
                "type {} (lines {}-{})",
                def.name, def.start_line, def.end_line
            ),
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
        edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Calls)
            .count(),
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
                            if let Some(caller_id) = find_enclosing_function_id(child, funcs) {
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
    funcs
        .iter()
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
        name_to_ids
            .entry(node.label.as_str())
            .or_default()
            .push(node.id.as_str());
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
  --bg:#f0f4f8;--surface:#ffffff;--surface2:#f7f9fc;
  --border:rgba(0,0,0,.09);--border-hi:rgba(59,130,246,.5);
  --text:#1e293b;--text-muted:#64748b;
  --accent-file:#059669;--accent-fn:#2563eb;--accent-type:#7c3aed;
  --accent-file-dim:rgba(5,150,105,.12);--accent-fn-dim:rgba(37,99,235,.12);--accent-type-dim:rgba(124,58,237,.12);
  --shadow:0 1px 4px rgba(0,0,0,.08),0 4px 16px rgba(0,0,0,.06);
  --handle:5px;
  font-family:'Inter',ui-sans-serif,system-ui,sans-serif;
}}
html,body{{height:100%;overflow:hidden;background:var(--bg);color:var(--text);font-size:13px;line-height:1.55}}
::-webkit-scrollbar{{width:6px;height:6px}}
::-webkit-scrollbar-track{{background:transparent}}
::-webkit-scrollbar-thumb{{background:rgba(0,0,0,.18);border-radius:3px}}
::-webkit-scrollbar-thumb:hover{{background:rgba(0,0,0,.28)}}

/* ── layout ─────────────────────────────────────────────────────────── */
#app{{display:flex;flex-direction:row;height:100vh;overflow:hidden}}

/* sidebar base */
.sidebar{{display:flex;flex-direction:column;background:var(--surface);flex-shrink:0;overflow:hidden;transition:width .22s cubic-bezier(.4,0,.2,1);box-shadow:var(--shadow)}}
#sidebar-left{{width:300px;min-width:0;border-right:1px solid var(--border);z-index:2}}
#sidebar-right{{width:340px;min-width:0;border-left:1px solid var(--border);z-index:2}}

/* collapsed state – a slim icon-strip that is ALWAYS clickable to expand */
.sidebar.collapsed{{width:40px!important}}
.sidebar.collapsed .sidebar-body{{display:none}}
.sidebar.collapsed .sidebar-full-content{{display:none!important}}
.sidebar.collapsed .sidebar-header{{padding:8px 0;align-items:center;border-bottom:none}}
.sidebar.collapsed .sidebar-collapse-btn{{width:40px;height:100%}}

/* graph area */
#graph-area{{flex:1 1 0;min-width:0;position:relative;overflow:hidden;background:linear-gradient(160deg,#e8f0fe 0%,#f0f4f8 50%,#e8f4ed 100%)}}

/* resize handles */
.handle{{width:var(--handle);cursor:col-resize;flex-shrink:0;background:var(--border);transition:background .15s;display:flex;align-items:center;justify-content:center;z-index:3}}
.handle:hover,.handle.dragging{{background:var(--border-hi)}}
.handle::after{{content:'';display:block;width:2px;height:28px;border-radius:2px;background:rgba(0,0,0,.15)}}

/* sidebar inner */
.sidebar-header{{padding:14px 14px 10px;border-bottom:1px solid var(--border);flex-shrink:0;display:flex;flex-direction:column;gap:0}}
.sidebar-header-row{{display:flex;align-items:center;gap:7px}}
.sidebar-body{{flex:1 1 0;overflow-y:auto;overflow-x:hidden;padding:14px;display:flex;flex-direction:column;gap:10px}}
.logo{{display:flex;align-items:center;gap:8px;margin-bottom:4px}}
.logo-icon{{width:26px;height:26px;border-radius:8px;background:linear-gradient(135deg,#2563eb 0%,#7c3aed 100%);display:flex;align-items:center;justify-content:center;font-size:14px;color:#fff;flex-shrink:0}}
.logo-text{{font-size:14px;font-weight:700;letter-spacing:-.3px;color:var(--text)}}
.tagline{{color:var(--text-muted);font-size:11px;margin-top:1px}}
.sidebar-collapse-btn{{background:none;border:none;color:var(--text-muted);cursor:pointer;padding:5px;border-radius:7px;display:flex;align-items:center;justify-content:center;transition:color .15s,background .15s;flex-shrink:0;font-size:15px;line-height:1}}
.sidebar-collapse-btn:hover{{color:var(--text);background:rgba(0,0,0,.06)}}

/* stats */
.stats{{display:grid;grid-template-columns:repeat(4,1fr);gap:5px}}
.stat{{background:var(--surface2);border:1px solid var(--border);border-radius:9px;padding:8px 4px;text-align:center}}
.stat-value{{font-size:16px;font-weight:700;line-height:1}}
.stat-label{{font-size:9px;color:var(--text-muted);text-transform:uppercase;letter-spacing:.06em;margin-top:2px}}
.stat.file .stat-value{{color:var(--accent-file)}}
.stat.fn .stat-value{{color:var(--accent-fn)}}
.stat.type .stat-value{{color:var(--accent-type)}}
.stat.calls .stat-value{{color:#ea580c}}

/* search / filter */
.search-wrap{{position:relative}}
.search-icon{{position:absolute;left:11px;top:50%;transform:translateY(-50%);opacity:.4;pointer-events:none;font-size:13px}}
input#search{{width:100%;padding:9px 12px 9px 32px;border-radius:10px;border:1px solid var(--border);background:var(--surface2);color:var(--text);font-size:13px;outline:none;transition:border-color .15s,box-shadow .15s}}
input#search:focus{{border-color:var(--border-hi);box-shadow:0 0 0 3px rgba(37,99,235,.12)}}
select{{width:100%;padding:8px 10px;border-radius:10px;border:1px solid var(--border);background:var(--surface2);color:var(--text);font-size:12px;outline:none;cursor:pointer;appearance:none;background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='8' viewBox='0 0 12 8'%3E%3Cpath fill='%2364748b' d='M6 8 0 0h12z'/%3E%3C/svg%3E");background-repeat:no-repeat;background-position:right 10px center}}

/* result list */
.results-count{{font-size:11px;color:var(--text-muted);padding:2px 0 4px}}
.item-list{{display:flex;flex-direction:column;gap:5px}}
button.item{{text-align:left;border:1px solid var(--border);background:var(--surface2);color:var(--text);padding:9px 11px;border-radius:10px;cursor:pointer;transition:border-color .12s,background .12s,box-shadow .12s;width:100%;min-width:0}}
button.item:hover{{border-color:rgba(37,99,235,.35);background:#eff6ff;box-shadow:0 2px 8px rgba(37,99,235,.08)}}
button.item.active{{border-color:var(--border-hi);background:#eff6ff;box-shadow:0 2px 8px rgba(37,99,235,.12)}}
button.item.active-file{{border-color:rgba(5,150,105,.4);background:#ecfdf5}}
button.item.active-type{{border-color:rgba(124,58,237,.4);background:#f5f3ff}}
.item-top{{display:flex;align-items:center;gap:6px;margin-bottom:3px;min-width:0}}
/* FIX: clamp name so it never overflows */
.item-name{{font-weight:600;font-size:13px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1;min-width:0}}
.item-path{{font-size:11px;color:var(--text-muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}
.item-summary{{font-size:11.5px;color:var(--text-muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;margin-top:2px}}

/* pill badges */
.pill{{display:inline-block;font-size:10px;font-weight:600;text-transform:uppercase;letter-spacing:.05em;padding:2px 7px;border-radius:999px;flex-shrink:0}}
.pill-file{{background:var(--accent-file-dim);color:var(--accent-file)}}
.pill-function{{background:var(--accent-fn-dim);color:var(--accent-fn)}}
.pill-type{{background:var(--accent-type-dim);color:var(--accent-type)}}

/* graph canvas */
#graph-canvas{{position:absolute;inset:0;touch-action:none}}

/* floating toolbar */
#graph-toolbar{{position:absolute;top:14px;right:14px;display:flex;flex-direction:column;gap:6px;z-index:10;user-select:none}}
.tb-group{{display:flex;flex-direction:column;background:rgba(255,255,255,.92);border:1px solid var(--border);border-radius:12px;overflow:hidden;box-shadow:var(--shadow)}}
.tb-btn{{background:none;border:none;color:var(--text-muted);cursor:pointer;padding:8px 12px;font-size:15px;line-height:1;transition:color .12s,background .12s;display:flex;align-items:center;justify-content:center;min-width:38px;gap:5px}}
.tb-btn:hover{{color:var(--text);background:rgba(37,99,235,.07)}}
.tb-btn + .tb-btn{{border-top:1px solid var(--border)}}
.tb-btn span.tb-label{{font-size:10px;font-weight:600;color:var(--text-muted);text-transform:uppercase;letter-spacing:.05em}}
#link-len-wrap{{background:rgba(255,255,255,.92);border:1px solid var(--border);border-radius:12px;padding:8px 12px;box-shadow:var(--shadow);display:flex;align-items:center;gap:8px}}
#link-len-wrap label{{font-size:10px;color:var(--text-muted);text-transform:uppercase;letter-spacing:.05em;white-space:nowrap;font-weight:600}}
input[type=range]#link-len{{-webkit-appearance:none;appearance:none;width:80px;height:4px;border-radius:2px;background:#e2e8f0;outline:none;cursor:pointer}}
input[type=range]#link-len::-webkit-slider-thumb{{-webkit-appearance:none;width:14px;height:14px;border-radius:50%;background:var(--accent-fn);cursor:pointer;box-shadow:0 1px 4px rgba(37,99,235,.4)}}

/* rotate hint badge */
#rotate-hint{{position:absolute;bottom:50px;right:14px;font-size:10px;color:var(--text-muted);background:rgba(255,255,255,.82);border:1px solid var(--border);border-radius:8px;padding:4px 8px;pointer-events:none;box-shadow:var(--shadow)}}

/* tooltip */
#tooltip{{position:fixed;pointer-events:none;background:rgba(15,23,42,.88);border-radius:9px;padding:7px 12px;font-size:12px;color:#f8fafc;white-space:nowrap;display:none;z-index:99;box-shadow:0 4px 16px rgba(0,0,0,.22)}}

/* legend */
.graph-legend{{position:absolute;bottom:14px;left:50%;transform:translateX(-50%);display:flex;gap:14px;background:rgba(255,255,255,.88);border:1px solid var(--border);border-radius:99px;padding:5px 18px;box-shadow:var(--shadow);pointer-events:none}}
.legend-item{{display:flex;align-items:center;gap:5px;font-size:11px;color:var(--text-muted)}}
.legend-dot{{width:9px;height:9px;border-radius:50%}}

/* inspector */
.detail-placeholder{{display:flex;flex-direction:column;align-items:center;justify-content:center;height:100%;text-align:center;padding:32px;gap:10px;opacity:.45}}
.detail-section{{margin-bottom:16px}}
.detail-section h3{{font-size:11px;text-transform:uppercase;letter-spacing:.08em;color:var(--text-muted);margin-bottom:6px;font-weight:600}}
.detail-path{{font-size:11px;color:var(--text-muted);font-family:ui-monospace,monospace;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}
.doc-box{{white-space:pre-wrap;background:var(--surface2);border:1px solid var(--border);border-radius:10px;padding:11px 13px;line-height:1.55;font-family:ui-monospace,monospace;font-size:12px;color:var(--text)}}
.code-box{{white-space:pre;overflow:auto;background:#0f172a;border:1px solid var(--border);border-radius:10px;padding:11px 13px;font-family:ui-monospace,monospace;font-size:12px;line-height:1.45;max-height:40vh;color:#e2e8f0}}
.rel-list{{list-style:none;display:flex;flex-direction:column;gap:4px}}
.rel-item{{display:flex;align-items:center;gap:7px;padding:7px 10px;border-radius:8px;background:var(--surface2);border:1px solid var(--border);font-size:12px;cursor:pointer;transition:border-color .12s,background .12s;min-width:0}}
.rel-item:hover{{border-color:var(--border-hi);background:#eff6ff}}
.rel-item-name{{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1;min-width:0}}
.rel-arrow{{color:var(--text-muted);flex-shrink:0}}
.rel-label{{font-size:10px;text-transform:uppercase;letter-spacing:.05em;color:var(--text-muted);flex-shrink:0}}
.callsite{{padding:7px 10px;border-radius:8px;background:var(--surface2);border:1px solid var(--border);font-size:11px;cursor:pointer;transition:border-color .12s,background .12s}}
.callsite:hover{{border-color:var(--border-hi);background:#eff6ff}}
.callsite-loc{{color:var(--text-muted);font-family:ui-monospace,monospace}}
.callsite-snippet{{font-family:ui-monospace,monospace;color:var(--text-muted);margin-top:2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}}
#back-btn{{background:none;border:1px solid var(--border);color:var(--text-muted);cursor:pointer;padding:4px 9px;border-radius:7px;font-size:11px;display:none;align-items:center;gap:4px;transition:border-color .12s,background .12s;white-space:nowrap;flex-shrink:0}}
#back-btn:hover{{border-color:var(--border-hi);color:var(--text);background:#eff6ff}}
#back-btn.visible{{display:flex}}
.insp-title{{font-size:12px;font-weight:700;color:var(--text-muted);text-transform:uppercase;letter-spacing:.07em}}
</style>
</head>
<body>
<div id="app">

<!-- ─── LEFT SIDEBAR ──────────────────────────────────────────────────── -->
<aside id="sidebar-left" class="sidebar">
  <div class="sidebar-header">
    <div class="sidebar-header-row">
      <button class="sidebar-collapse-btn" id="collapse-left" title="Collapse / expand">◀</button>
      <div class="sidebar-full-content" style="flex:1;min-width:0">
        <div class="logo"><div class="logo-icon">⬡</div><span class="logo-text">Codebase Visualizer</span></div>
        <div class="tagline">Call graph · types · relationships</div>
      </div>
    </div>
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

<!-- ─── GRAPH AREA ────────────────────────────────────────────────────── -->
<div id="graph-area">
  <canvas id="graph-canvas" aria-label="Codebase relationship graph"></canvas>
  <div id="tooltip"></div>
  <div id="rotate-hint">Ctrl + drag to rotate</div>

  <div id="graph-toolbar">
    <div class="tb-group">
      <button class="tb-btn" id="btn-fit"    title="Fit all in view (F)">⊙</button>
      <button class="tb-btn" id="btn-zoomin" title="Zoom in (+ / scroll up)">＋</button>
      <button class="tb-btn" id="btn-zoomout"title="Zoom out (− / scroll down)">－</button>
      <button class="tb-btn" id="btn-reset"  title="Reset rotation">↺</button>
    </div>
    <div id="link-len-wrap">
      <label for="link-len">Link</label>
      <input type="range" id="link-len" min="20" max="300" value="50" step="5"/>
    </div>
  </div>

  <div class="graph-legend">
    <div class="legend-item"><div class="legend-dot" style="background:#059669"></div>File</div>
    <div class="legend-item"><div class="legend-dot" style="background:#2563eb"></div>Function</div>
    <div class="legend-item"><div class="legend-dot" style="background:#7c3aed"></div>Type</div>
  </div>
</div>

<div class="handle" id="handle-right" title="Drag to resize"></div>

<!-- ─── RIGHT SIDEBAR ────────────────────────────────────────────────── -->
<aside id="sidebar-right" class="sidebar">
  <div class="sidebar-header">
    <div class="sidebar-header-row" style="justify-content:space-between">
      <button class="sidebar-collapse-btn" id="collapse-right" title="Collapse / expand">▶</button>
      <div class="sidebar-full-content" style="display:flex;align-items:center;gap:8px;flex:1;min-width:0;justify-content:space-between">
        <span class="insp-title">Inspector</span>
        <button id="back-btn" title="Go back">← <span id="back-label"></span></button>
      </div>
    </div>
  </div>
  <div class="sidebar-body" id="details-body">
    <div class="detail-placeholder">
      <svg width="44" height="44" viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="2"><circle cx="24" cy="24" r="20"/><circle cx="24" cy="24" r="7"/><line x1="24" y1="4" x2="24" y2="17"/><line x1="24" y1="31" x2="24" y2="44"/><line x1="4" y1="24" x2="17" y2="24"/><line x1="31" y1="24" x2="44" y2="24"/></svg>
      <div style="font-weight:600">Select a node</div>
      <div style="font-size:12px">Click any node in the graph<br>or pick one from the list</div>
    </div>
  </div>
</aside>

</div><!-- #app -->

<script>
/* ═══ DATA ══════════════════════════════════════════════════════════════ */
const GRAPH  = {graph_json};
const COLORS = {{ file:'#059669', function:'#2563eb', type:'#7c3aed' }};
const RADII  = {{ file:12, function:9, type:10 }};

/* ═══ CAMERA ════════════════════════════════════════════════════════════
   All graph coordinates are in "world space".
   The camera transforms them to screen space with:
     screen = rotate(world - worldCenter, angle) * scale + screenCenter
   This means zoom is "scale the world around its own centre",
   pan is "shift the world centre", rotate is "spin the world".        */
const CAM = {{ tx:0, ty:0, scale:1, angle:0 }};

function worldToScreen(wx, wy){{
  const cosA=Math.cos(CAM.angle),sinA=Math.sin(CAM.angle);
  const rx=wx*cosA - wy*sinA;
  const ry=wx*sinA + wy*cosA;
  return {{x: rx*CAM.scale + canvas.width/2  + CAM.tx,
           y: ry*CAM.scale + canvas.height/2 + CAM.ty}};
}}
function screenToWorld(sx, sy){{
  const dx=(sx - canvas.width/2  - CAM.tx)/CAM.scale;
  const dy=(sy - canvas.height/2 - CAM.ty)/CAM.scale;
  const cosA=Math.cos(-CAM.angle),sinA=Math.sin(-CAM.angle);
  return {{x: dx*cosA - dy*sinA, y: dx*sinA + dy*cosA}};
}}

/* ═══ STATE ══════════════════════════════════════════════════════════════ */
let K_REST  = 50;
const K_SPRING = 0.025, K_REPEL = 5000, K_DAMP = 0.78, CENTER_K = 0.02;

const S = {{
  selected:null, history:[], visible:[], sim:[], edgesVis:[],
  draggingNode:null,
  gesture: null,    // {{ mode:'pan'|'rotate'|'pinch', lx,ly, touches }}
  hot:null, dirty:true,
  leftCollapsed:false, rightCollapsed:false,
  leftW:300, rightW:340,
}};

/* ═══ DOM ════════════════════════════════════════════════════════════════ */
const searchEl    = document.getElementById('search');
const viewModeEl  = document.getElementById('viewMode');
const resultsCt   = document.getElementById('results');
const countEl     = document.getElementById('results-count');
const detailsCt   = document.getElementById('details-body');
const canvas      = document.getElementById('graph-canvas');
const tip         = document.getElementById('tooltip');
const ctx         = canvas.getContext('2d');
const sidebarLeft = document.getElementById('sidebar-left');
const sidebarRight= document.getElementById('sidebar-right');
const backBtn     = document.getElementById('back-btn');
const backLabel   = document.getElementById('back-label');
const linkLenInput= document.getElementById('link-len');

/* ═══ URL PARAMS ══════════════════════════════════════════════════════════ */
const params    = new URLSearchParams(window.location.search);
const reqSelect = params.get('select');
if (params.get('q')) searchEl.value = params.get('q');
if (['callgraph','types'].includes(params.get('view'))) viewModeEl.value = params.get('view');

/* ═══ STATS ═══════════════════════════════════════════════════════════════ */
document.getElementById('stats').innerHTML=[
  ['file','file',GRAPH.stats.files||0,'Files'],
  ['fn','function',GRAPH.stats.functions||0,'Funcs'],
  ['type','type',GRAPH.stats.types||0,'Types'],
  ['calls','calls',GRAPH.stats.calls||0,'Calls'],
].map(([c,_,v,l])=>`<div class="stat ${{c}}"><div class="stat-value">${{v}}</div><div class="stat-label">${{l}}</div></div>`).join('');

/* ═══ DATASET ════════════════════════════════════════════════════════════ */
function buildAdj(links){{
  const out=new Map(),inc=new Map();
  for(const l of links){{
    if(!out.has(l.source))out.set(l.source,new Set());
    if(!inc.has(l.target))inc.set(l.target,new Set());
    out.get(l.source).add(l.target);inc.get(l.target).add(l.source);
  }}
  return{{out,inc}};
}}
function dataset(){{
  const m=viewModeEl.value;
  if(m==='callgraph')return{{nodes:GRAPH.nodes.filter(n=>n.kind==='function'),links:GRAPH.edges.filter(e=>e.kind==='calls')}};
  const tids=new Set(GRAPH.nodes.filter(n=>n.kind==='type').map(n=>n.id));
  return{{nodes:GRAPH.nodes.filter(n=>n.kind==='file'||n.kind==='type'),links:GRAPH.edges.filter(e=>e.kind==='contains'&&tids.has(e.target))}};
}}
function filteredDataset(){{
  const q=searchEl.value.trim().toLowerCase(),base=dataset();
  if(!q)return base;
  const {{out,inc}}=buildAdj(base.links);
  const keep=new Set();
  for(const n of base.nodes){{
    if((n.label+' '+n.path+' '+n.doc+' '+n.summary).toLowerCase().includes(q)){{
      keep.add(n.id);
      for(const nb of(out.get(n.id)||[]))keep.add(nb);
      for(const nb of(inc.get(n.id)||[]))keep.add(nb);
    }}
  }}
  const nodes=base.nodes.filter(n=>keep.has(n.id));
  const ks=new Set(nodes.map(n=>n.id));
  return{{nodes,links:base.links.filter(l=>ks.has(l.source)&&ks.has(l.target))}};
}}

/* ═══ SIMULATION (world space) ════════════════════════════════════════════ */
function initSim(){{
  const byId=new Map(S.sim.map(n=>[n.id,n]));
  const ds=filteredDataset();
  S.visible=ds.nodes; S.edgesVis=ds.links;
  const R=Math.min(canvas.width,canvas.height)/(CAM.scale*3)+40;
  S.sim=S.visible.map((node,i)=>{{
    const old=byId.get(node.id);
    if(old)return{{...old,node}};
    const angle=Math.PI*2*i/Math.max(S.visible.length,1);
    const r=R*(0.5+Math.random()*0.5);
    return{{id:node.id,node,x:Math.cos(angle)*r,y:Math.sin(angle)*r,vx:0,vy:0}};
  }});
  S.dirty=true;
}}

function tick(){{
  if(S.draggingNode)return;
  let moving=false;
  for(let i=0;i<S.sim.length;i++){{
    const a=S.sim[i];
    for(let j=i+1;j<S.sim.length;j++){{
      const b=S.sim[j],dx=a.x-b.x,dy=a.y-b.y,d2=dx*dx+dy*dy+1,f=K_REPEL/d2;
      a.vx+=f*dx;a.vy+=f*dy;b.vx-=f*dx;b.vy-=f*dy;
    }}
  }}
  const pm=new Map(S.sim.map(n=>[n.id,n]));
  for(const e of S.edgesVis){{
    const a=pm.get(e.source),b=pm.get(e.target);if(!a||!b)continue;
    const dx=b.x-a.x,dy=b.y-a.y,dist=Math.sqrt(dx*dx+dy*dy)+.01,f=K_SPRING*(dist-K_REST);
    a.vx+=f*dx/dist;a.vy+=f*dy/dist;b.vx-=f*dx/dist;b.vy-=f*dy/dist;
  }}
  for(const n of S.sim){{
    n.vx+=-n.x*CENTER_K;n.vy+=-n.y*CENTER_K;
    n.vx*=K_DAMP;n.vy*=K_DAMP;n.x+=n.vx;n.y+=n.vy;
    if(Math.abs(n.vx)>.04||Math.abs(n.vy)>.04)moving=true;
  }}
  if(moving)S.dirty=true;
}}

/* ═══ FIT ════════════════════════════════════════════════════════════════ */
function fitAll(){{
  if(!S.sim.length)return;
  let minX=Infinity,maxX=-Infinity,minY=Infinity,maxY=-Infinity;
  for(const n of S.sim){{minX=Math.min(minX,n.x);maxX=Math.max(maxX,n.x);minY=Math.min(minY,n.y);maxY=Math.max(maxY,n.y);}}
  const W=canvas.width,H=canvas.height,pad=60;
  const gW=maxX-minX||1,gH=maxY-minY||1;
  CAM.scale=Math.min((W-pad*2)/gW,(H-pad*2)/gH,3);
  CAM.tx=0;CAM.ty=0;
  S.dirty=true;
}}

/* ═══ DRAWING ════════════════════════════════════════════════════════════ */
function draw(){{
  const W=canvas.width,H=canvas.height;
  ctx.clearRect(0,0,W,H);
  // faint dot grid in screen space
  ctx.save();
  ctx.fillStyle='rgba(148,163,184,.18)';
  const gs=40;
  for(let x=gs/2;x<W;x+=gs)for(let y=gs/2;y<H;y+=gs){{ctx.beginPath();ctx.arc(x,y,1.2,0,Math.PI*2);ctx.fill();}}
  ctx.restore();

  const pm=new Map(S.sim.map(n=>[n.id,n]));
  // edges
  for(const e of S.edgesVis){{
    const a=pm.get(e.source),b=pm.get(e.target);if(!a||!b)continue;
    const sa=worldToScreen(a.x,a.y),sb=worldToScreen(b.x,b.y);
    const rel=S.selected&&(e.source===S.selected||e.target===S.selected);
    ctx.beginPath();ctx.moveTo(sa.x,sa.y);ctx.lineTo(sb.x,sb.y);
    ctx.strokeStyle=rel?'rgba(37,99,235,.55)':'rgba(100,116,139,.22)';
    ctx.lineWidth=rel?2.2:1;ctx.stroke();
  }}
  // nodes
  for(const n of S.sim){{
    const r=(RADII[n.node.kind]||8)*Math.max(0.5,Math.min(CAM.scale,2));
    const col=COLORS[n.node.kind]||'#64748b';
    const sp=worldToScreen(n.x,n.y);
    const isSel=n.id===S.selected,isHot=n.id===S.hot;
    const isRel=S.selected&&S.edgesVis.some(e=>(e.source===S.selected&&e.target===n.id)||(e.target===S.selected&&e.source===n.id));
    ctx.save();
    // shadow
    if(isSel||isHot){{ctx.shadowColor=col;ctx.shadowBlur=isSel?24:12;}}
    else if(isRel){{ctx.shadowColor=col;ctx.shadowBlur=10;}}
    // selection ring
    if(isSel){{
      ctx.beginPath();ctx.arc(sp.x,sp.y,r+7,0,Math.PI*2);
      ctx.strokeStyle=col;ctx.globalAlpha=.3;ctx.lineWidth=3;ctx.stroke();ctx.globalAlpha=1;
    }}
    // circle fill
    ctx.beginPath();ctx.arc(sp.x,sp.y,r,0,Math.PI*2);
    if(isSel||isHot){{
      ctx.fillStyle=col;
    }}else if(isRel){{
      ctx.fillStyle=col;ctx.globalAlpha=.75;
    }}else{{
      ctx.globalAlpha=S.selected?.28:1;
      // white core + color edge
      const g=ctx.createRadialGradient(sp.x-r*.3,sp.y-r*.3,r*.1,sp.x,sp.y,r);
      g.addColorStop(0,col+'cc');g.addColorStop(1,col);
      ctx.fillStyle=g;
    }}
    ctx.fill();ctx.globalAlpha=1;ctx.shadowBlur=0;
    // label (skip tiny nodes)
    if(r>3){{
      const fs=Math.max(9,Math.min(13,r*1.1));
      ctx.font=`${{(isSel||isHot)?'600 ':'400 '}}${{fs}}px Inter,sans-serif`;
      ctx.fillStyle=(S.selected&&!isSel&&!isRel)?'rgba(100,116,139,.4)':'#1e293b';
      ctx.textBaseline='middle';ctx.fillText(n.node.label,sp.x+r+4,sp.y);
    }}
    ctx.restore();
  }}
}}

function loop(){{tick();if(S.dirty){{draw();S.dirty=false;}}requestAnimationFrame(loop);}}

function resizeCanvas(){{
  const a=document.getElementById('graph-area');
  canvas.width=a.clientWidth;canvas.height=a.clientHeight;S.dirty=true;
}}

/* ═══ HIT TEST (screen→world→check) ══════════════════════════════════════ */
function hitTest(sx,sy){{
  const w=screenToWorld(sx,sy);
  let best=null,bestD=Infinity;
  for(const n of S.sim){{
    const dx=n.x-w.x,dy=n.y-w.y,d=dx*dx+dy*dy;
    const rW=(RADII[n.node.kind]||8)+5/CAM.scale;
    if(d<=rW*rW&&d<bestD){{best=n;bestD=d;}}
  }}
  return best;
}}

/* ═══ POINTER EVENTS ══════════════════════════════════════════════════════ */
/* Mouse */
canvas.addEventListener('mousedown',e=>{{
  const n=hitTest(e.offsetX,e.offsetY);
  if(n){{S.draggingNode=n;selectNode(n.id,true);}}
  else{{
    const mode=e.ctrlKey||e.metaKey?'rotate':'pan';
    S.gesture={{mode,lx:e.clientX,ly:e.clientY}};
    canvas.style.cursor=mode==='rotate'?'crosshair':'grabbing';
  }}
}});
canvas.addEventListener('mousemove',e=>{{
  const n=hitTest(e.offsetX,e.offsetY);
  if(n!==null?n.id:null !==S.hot){{S.hot=n?n.id:null;S.dirty=true;}}
  if(S.draggingNode){{
    const w=screenToWorld(e.offsetX,e.offsetY);
    S.draggingNode.x=w.x;S.draggingNode.y=w.y;
    S.draggingNode.vx=0;S.draggingNode.vy=0;S.dirty=true;
  }}else if(S.gesture){{
    const dx=e.clientX-S.gesture.lx,dy=e.clientY-S.gesture.ly;
    if(S.gesture.mode==='pan'){{CAM.tx+=dx;CAM.ty+=dy;}}
    else{{CAM.angle+=dx*0.008;}}
    S.gesture.lx=e.clientX;S.gesture.ly=e.clientY;S.dirty=true;
  }}
  // tooltip
  if(n){{
    tip.style.display='block';
    tip.style.left=(e.clientX+14)+'px';tip.style.top=(e.clientY-8)+'px';
    tip.textContent=`${{n.node.kind.toUpperCase()}}  ${{n.node.label}}  ·  ${{n.node.path}}:${{n.node.line}}`;
    canvas.style.cursor='pointer';
  }}else if(!S.gesture){{
    tip.style.display='none';
    canvas.style.cursor=e.ctrlKey||e.metaKey?'crosshair':'grab';
  }}else{{tip.style.display='none';}}
}});
window.addEventListener('mouseup',()=>{{
  S.draggingNode=null;S.gesture=null;
  canvas.style.cursor='grab';
}});
// key modifier: update cursor live
window.addEventListener('keydown',e=>{{if(e.ctrlKey||e.metaKey)canvas.style.cursor='crosshair';}});
window.addEventListener('keyup',  e=>{{if(!e.ctrlKey&&!e.metaKey)canvas.style.cursor='grab';}});

/* Scroll wheel zoom (around cursor) */
canvas.addEventListener('wheel',e=>{{
  e.preventDefault();
  const f=e.deltaY<0?1.12:.90;
  const wx=e.offsetX,wy=e.offsetY;
  // zoom toward cursor: adjust tx,ty so the world-point under the cursor stays fixed
  CAM.tx=(CAM.tx-wx)*f+wx;
  CAM.ty=(CAM.ty-wy)*f+wy;
  CAM.scale*=f;
  S.dirty=true;
}},{{passive:false}});

/* Touch pinch / pan / rotate */
canvas.addEventListener('touchstart',e=>{{
  e.preventDefault();
  if(e.touches.length===1){{
    const t=e.touches[0];
    S.gesture={{mode:'pan',lx:t.clientX,ly:t.clientY}};
  }}else if(e.touches.length===2){{
    S.gesture={{
      mode:'pinch',
      lx:(e.touches[0].clientX+e.touches[1].clientX)/2,
      ly:(e.touches[0].clientY+e.touches[1].clientY)/2,
      dist:Math.hypot(e.touches[0].clientX-e.touches[1].clientX,
                      e.touches[0].clientY-e.touches[1].clientY),
      angle:Math.atan2(e.touches[1].clientY-e.touches[0].clientY,
                       e.touches[1].clientX-e.touches[0].clientX),
    }};
  }}
}},{{passive:false}});
canvas.addEventListener('touchmove',e=>{{
  e.preventDefault();
  if(!S.gesture)return;
  if(e.touches.length===1&&S.gesture.mode==='pan'){{
    const t=e.touches[0];
    CAM.tx+=t.clientX-S.gesture.lx;CAM.ty+=t.clientY-S.gesture.ly;
    S.gesture.lx=t.clientX;S.gesture.ly=t.clientY;S.dirty=true;
  }}else if(e.touches.length===2&&S.gesture.mode==='pinch'){{
    const mx=(e.touches[0].clientX+e.touches[1].clientX)/2;
    const my=(e.touches[0].clientY+e.touches[1].clientY)/2;
    const dist=Math.hypot(e.touches[0].clientX-e.touches[1].clientX,
                          e.touches[0].clientY-e.touches[1].clientY);
    const ang=Math.atan2(e.touches[1].clientY-e.touches[0].clientY,
                         e.touches[1].clientX-e.touches[0].clientX);
    const f=dist/S.gesture.dist;
    CAM.tx=(CAM.tx-mx)*f+mx;CAM.ty=(CAM.ty-my)*f+my;
    CAM.scale*=f;
    CAM.angle+=ang-S.gesture.angle;
    S.gesture.dist=dist;S.gesture.angle=ang;S.gesture.lx=mx;S.gesture.ly=my;
    S.dirty=true;
  }}
}},{{passive:false}});
canvas.addEventListener('touchend',e=>{{if(e.touches.length===0)S.gesture=null;}},{{passive:false}});

/* keyboard shortcuts */
window.addEventListener('keydown',e=>{{
  if(e.target!==document.body&&e.target!==canvas)return;
  if(e.key==='f'||e.key==='F')fitAll();
  if(e.key==='+'||e.key==='='){{CAM.scale*=1.2;S.dirty=true;}}
  if(e.key==='-'){{CAM.scale*=0.83;S.dirty=true;}}
  if(e.key==='r'||e.key==='R'){{CAM.angle=0;S.dirty=true;}}
}});

/* ═══ TOOLBAR BUTTONS ════════════════════════════════════════════════════ */
document.getElementById('btn-fit').addEventListener('click',fitAll);
document.getElementById('btn-zoomin').addEventListener('click',()=>{{CAM.scale*=1.3;S.dirty=true;}});
document.getElementById('btn-zoomout').addEventListener('click',()=>{{CAM.scale*=0.77;S.dirty=true;}});
document.getElementById('btn-reset').addEventListener('click',()=>{{CAM.angle=0;S.dirty=true;}});
linkLenInput.addEventListener('input',()=>{{K_REST=+linkLenInput.value;S.dirty=true;}});

/* ═══ NAVIGATION HISTORY ══════════════════════════════════════════════════ */
function selectNode(id, fromGraph){{
  if(S.selected&&S.selected!==id){{S.history.push(S.selected);if(S.history.length>30)S.history.shift();}}
  S.selected=id;S.dirty=true;
  if(fromGraph&&S.rightCollapsed){{
    S.rightCollapsed=false;sidebarRight.classList.remove('collapsed');
    sidebarRight.style.width=S.rightW+'px';
    document.getElementById('collapse-right').textContent='▶';
    resizeCanvas();
  }}
  renderDetails();renderResults();updateBackBtn();
}}
function goBack(){{
  if(!S.history.length)return;
  S.selected=S.history.pop();S.dirty=true;
  renderDetails();renderResults();updateBackBtn();
}}
function updateBackBtn(){{
  if(S.history.length){{
    const n=GRAPH.nodes.find(n=>n.id===S.history[S.history.length-1]);
    backLabel.textContent=n?n.label:'…';backBtn.classList.add('visible');
  }}else backBtn.classList.remove('visible');
}}
backBtn.addEventListener('click',goBack);

/* ═══ SIDEBAR COLLAPSE ════════════════════════════════════════════════════ */
function setupCollapse(btnId, sidebarEl, side){{
  const btn=document.getElementById(btnId);
  btn.addEventListener('click',()=>{{
    const c=sidebarEl.classList.toggle('collapsed');
    if(side==='left'){{S.leftCollapsed=c;if(!c)sidebarEl.style.width=S.leftW+'px';btn.textContent=c?'▶':'◀';}}
    else{{S.rightCollapsed=c;if(!c)sidebarEl.style.width=S.rightW+'px';btn.textContent=c?'◀':'▶';}}
    resizeCanvas();S.dirty=true;
  }});
}}
setupCollapse('collapse-left', sidebarLeft,'left');
setupCollapse('collapse-right',sidebarRight,'right');

/* ═══ RESIZE HANDLES ══════════════════════════════════════════════════════ */
function makeHandle(h,s,side){{
  let dr=false,sx=0,sw=0;
  h.addEventListener('mousedown',e=>{{
    if(side==='left'&&S.leftCollapsed)return;if(side==='right'&&S.rightCollapsed)return;
    dr=true;sx=e.clientX;sw=s.offsetWidth;h.classList.add('dragging');
    document.body.style.userSelect='none';document.body.style.cursor='col-resize';
  }});
  window.addEventListener('mousemove',e=>{{
    if(!dr)return;const d=side==='left'?e.clientX-sx:sx-e.clientX;
    const nw=Math.max(180,Math.min(600,sw+d));s.style.width=nw+'px';
    if(side==='left')S.leftW=nw;else S.rightW=nw;resizeCanvas();S.dirty=true;
  }});
  window.addEventListener('mouseup',()=>{{if(!dr)return;dr=false;h.classList.remove('dragging');document.body.style.userSelect='';document.body.style.cursor='';}});
}}
makeHandle(document.getElementById('handle-left'), sidebarLeft,'left');
makeHandle(document.getElementById('handle-right'),sidebarRight,'right');

/* ═══ HELPERS ════════════════════════════════════════════════════════════ */
function pillClass(k){{return k==='file'?'pill-file':k==='function'?'pill-function':'pill-type';}}
function activeClass(n){{
  if(S.selected!==n.id)return'';
  return n.kind==='file'?'active-file':n.kind==='type'?'active-type':'active';
}}
function escapeHtml(v){{return String(v).replace(/[&<>"']/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#039;'}}[c]));}}
function labelFor(id){{return GRAPH.nodes.find(n=>n.id===id)?.label||id;}}

/* ═══ RENDER RESULTS ══════════════════════════════════════════════════════ */
function renderResults(){{
  const ds=filteredDataset(),vis=ds.nodes.slice(0,100),tot=ds.nodes.length;
  countEl.textContent=tot===0?'No matches':`${{tot}} node${{tot===1?'':'s'}}${{tot>100?' (showing 100)':''}}`;
  resultsCt.innerHTML=vis.map(n=>`
    <button class="item ${{activeClass(n)}}" data-id="${{n.id}}">
      <div class="item-top"><span class="pill ${{pillClass(n.kind)}}">${{n.kind}}</span><span class="item-name">${{escapeHtml(n.label)}}</span></div>
      <div class="item-path">${{escapeHtml(n.path)}}:${{n.line}}</div>
      <div class="item-summary">${{escapeHtml(n.summary)}}</div>
    </button>`).join('');
  resultsCt.querySelectorAll('button').forEach(b=>b.addEventListener('click',()=>selectNode(b.dataset.id,false)));
}}

/* ═══ RENDER DETAILS ══════════════════════════════════════════════════════ */
function renderDetails(){{
  const id=S.selected,node=GRAPH.nodes.find(n=>n.id===id);
  if(!node){{
    detailsCt.innerHTML=`<div class="detail-placeholder">
      <svg width="44" height="44" viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="2"><circle cx="24" cy="24" r="20"/><circle cx="24" cy="24" r="7"/><line x1="24" y1="4" x2="24" y2="17"/><line x1="24" y1="31" x2="24" y2="44"/><line x1="4" y1="24" x2="17" y2="24"/><line x1="31" y1="24" x2="44" y2="24"/></svg>
      <div style="font-weight:600">Select a node</div>
      <div style="font-size:12px">Click any node in the graph<br>or pick one from the list</div></div>`;
    return;
  }}
  const ac=COLORS[node.kind]||'#64748b';
  const outE=GRAPH.edges.filter(e=>e.source===id&&e.kind==='calls');
  const inE =GRAPH.edges.filter(e=>e.target===id&&e.kind==='calls');
  const relE=GRAPH.edges.filter(e=>(e.source===id||e.target===id)&&e.kind!=='calls').slice(0,12);
  const cs  =inE.flatMap(e=>(e.callsites||[]).map(s=>Object.assign({{}},s,{{callerId:e.source}}))).slice(0,80);
  detailsCt.innerHTML=`
    <div style="padding-bottom:13px;margin-bottom:13px;border-bottom:1px solid var(--border)">
      <div style="display:flex;align-items:center;gap:7px;margin-bottom:5px">
        <span style="width:10px;height:10px;border-radius:50%;background:${{ac}};flex-shrink:0;box-shadow:0 0 8px ${{ac}}55"></span>
        <span class="pill ${{pillClass(node.kind)}}">${{node.kind}}</span>
        <span style="font-size:11px;color:var(--text-muted)">${{escapeHtml(node.language)}}</span>
      </div>
      <h2 style="font-size:15px;font-weight:700;word-break:break-word;color:var(--text)">${{escapeHtml(node.label)}}</h2>
      <div class="detail-path">${{escapeHtml(node.path)}}:${{node.line}}${{node.endLine&&node.endLine!==node.line?'–'+node.endLine:''}}</div>
    </div>
    ${{node.doc?`<div class="detail-section"><h3>Documentation</h3><div class="doc-box">${{escapeHtml(node.doc)}}</div></div>`:''}}
    ${{node.code?`<div class="detail-section"><h3>Code</h3><pre class="code-box">${{escapeHtml(node.code)}}</pre></div>`:''}}
    ${{cs.length?`<div class="detail-section"><h3>Called by (${{cs.length}})</h3><div style="display:flex;flex-direction:column;gap:4px">
      ${{cs.map(s=>`<div class="callsite" data-caller="${{escapeHtml(s.callerId)}}">
        <div><strong>${{escapeHtml(labelFor(s.callerId))}}</strong> <span class="callsite-loc">${{escapeHtml(s.path)}}:${{s.line}}:${{s.col}}</span></div>
        ${{s.snippet?`<div class="callsite-snippet">${{escapeHtml(s.snippet)}}</div>`:''}}</div>`).join('')}}</div></div>`:''}}
    ${{outE.length?`<div class="detail-section"><h3>Calls (${{outE.length}})</h3><ul class="rel-list">
      ${{outE.slice(0,30).map(e=>`<li class="rel-item" data-id="${{escapeHtml(e.target)}}">
        <span class="rel-arrow">→</span><span class="rel-item-name">${{escapeHtml(labelFor(e.target))}}</span>
        <span class="pill pill-function" style="flex-shrink:0">fn</span></li>`).join('')}}</ul></div>`:''}}
    ${{relE.length?`<div class="detail-section"><h3>Relationships (${{relE.length}})</h3><ul class="rel-list">
      ${{relE.map(e=>{{const o=e.source===id?e.target:e.source;const on=GRAPH.nodes.find(n=>n.id===o);
        return`<li class="rel-item" data-id="${{escapeHtml(o)}}">
          <span class="rel-label">${{escapeHtml(e.label)}}</span>
          <span class="rel-arrow">${{e.source===id?'→':'←'}}</span>
          <span class="rel-item-name">${{escapeHtml(on?.label||o)}}</span>
          ${{on?`<span class="pill ${{pillClass(on.kind)}}" style="flex-shrink:0">${{on.kind}}</span>`:''}}
        </li>`;
      }}).join('')}}</ul></div>`:''}}
  `;
  detailsCt.querySelectorAll('.callsite[data-caller]').forEach(el=>el.addEventListener('click',()=>selectNode(el.dataset.caller,false)));
  detailsCt.querySelectorAll('.rel-item[data-id]').forEach(el=>el.addEventListener('click',()=>selectNode(el.dataset.id,false)));
  detailsCt.scrollTop=0;
}}

/* ═══ FULL RENDER ════════════════════════════════════════════════════════ */
function fullRender(){{
  if(!S.selected&&reqSelect){{
    const req=reqSelect.toLowerCase();
    const hit=GRAPH.nodes.find(n=>n.label.toLowerCase()===req)||GRAPH.nodes.find(n=>n.id.toLowerCase().includes(req));
    if(hit)S.selected=hit.id;
  }}
  S.history=[];CAM.tx=0;CAM.ty=0;CAM.angle=0;
  initSim();renderResults();renderDetails();updateBackBtn();
  // fit after first paint
  requestAnimationFrame(()=>{{fitAll();
    let frames=0;function refit(){{frames++;if(frames===60){{fitAll();return;}}requestAnimationFrame(refit);}}
    requestAnimationFrame(refit);
  }});
}}

/* ═══ BOOT ════════════════════════════════════════════════════════════════ */
const ro=new ResizeObserver(()=>{{resizeCanvas();S.dirty=true;}});
ro.observe(document.getElementById('graph-area'));
resizeCanvas();
searchEl.addEventListener('input',fullRender);
viewModeEl.addEventListener('change',()=>{{S.selected=null;S.history=[];fullRender();}});
fullRender();loop();
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
