//! Throwaway spike: show what `ra_ap_syntax` actually hands back for a Rust file.
//!
//! Emits two views:
//!   * `<name>.cst.txt`  — the full-fidelity concrete syntax tree, everything
//!   * `<name>.tree.txt` — a projection down to definitions / calls / comments
//!
//! Node kinds are matched by their Debug name so this compiles regardless of
//! which `SyntaxKind` variants a given ra_ap_syntax release happens to expose.

use std::fmt::Write as _;

use ra_ap_syntax::{Edition, NodeOrToken, SourceFile, SyntaxNode, SyntaxToken, WalkEvent};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "sample.rs".to_string());
    let text = std::fs::read_to_string(&path).expect("read source file");

    // `--hold N`: keep N parsed trees alive so an external observer can read
    // this process's working set and derive the cost of retaining a tree.
    if let Some(n) = std::env::args().nth(2).and_then(|a| a.parse::<usize>().ok()) {
        let held: Vec<_> = (0..n).map(|_| SourceFile::parse(&text, Edition::CURRENT)).collect();
        println!("holding {} trees of {} bytes each", held.len(), text.len());
        std::thread::sleep(std::time::Duration::from_secs(6));
        std::hint::black_box(&held);
        return;
    }

    let started = std::time::Instant::now();
    let parse = SourceFile::parse(&text, Edition::CURRENT);
    let parse_time = started.elapsed();

    // Steady-state throughput, once caches and allocator are warm.
    let iters = 30u32;
    let bench_started = std::time::Instant::now();
    for _ in 0..iters {
        std::hint::black_box(SourceFile::parse(&text, Edition::CURRENT));
    }
    let steady = bench_started.elapsed() / iters;

    let errors = parse.errors();
    let root = parse.syntax_node();

    let lines = LineIndex::new(&text);
    let projection = project(&root, &lines);

    let out = std::path::Path::new("out");
    std::fs::create_dir_all(out).unwrap();
    let stem = std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "input".into());
    std::fs::write(out.join(format!("{stem}.cst.txt")), parse.debug_dump()).unwrap();
    std::fs::write(out.join(format!("{stem}.tree.txt")), &projection).unwrap();

    let total = root.descendants_with_tokens().count();
    let comments = root
        .descendants_with_tokens()
        .filter(|e| kind_of(e) == "COMMENT")
        .count();

    println!("=== {path} ===");
    println!("source bytes        : {}", text.len());
    println!("source lines        : {}", lines.count());
    println!("CST nodes + tokens  : {total}");
    println!("elements per byte   : {:.2}", total as f64 / text.len() as f64);
    println!("comment tokens      : {comments}");
    println!(
        "parse time (cold)   : {:.3} ms",
        parse_time.as_secs_f64() * 1000.0
    );
    println!(
        "parse time (warm)   : {:.3} ms  ({:.1} MB/s single core)",
        steady.as_secs_f64() * 1000.0,
        text.len() as f64 / steady.as_secs_f64() / 1_000_000.0
    );
    println!("syntax errors       : {}", errors.len());
    for e in errors.iter().take(8) {
        println!("    {:?}  {e}", e.range());
    }
    println!("wrote out/{stem}.cst.txt and out/{stem}.tree.txt");
    println!();
    println!("{projection}");
}

fn kind_of(element: &ra_ap_syntax::SyntaxElement) -> String {
    format!("{:?}", element.kind())
}

/// Byte offset -> 1-based line number.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        LineIndex { starts }
    }

    fn line(&self, offset: usize) -> usize {
        self.starts.partition_point(|&s| s <= offset)
    }

    fn count(&self) -> usize {
        self.starts.len()
    }
}

/// Walk the CST and print only the nodes that matter to the design, nested.
fn project(root: &SyntaxNode, lines: &LineIndex) -> String {
    let mut buf = String::new();
    let mut stack: Vec<SyntaxNode> = Vec::new();

    for event in root.preorder_with_tokens() {
        match event {
            WalkEvent::Enter(element) => match element {
                NodeOrToken::Node(node) => {
                    if let Some(label) = describe_node(&node, lines) {
                        let _ = writeln!(buf, "{:i$}{label}", "", i = stack.len() * 2);
                        stack.push(node);
                    }
                }
                NodeOrToken::Token(token) => {
                    if let Some(label) = describe_token(&token, lines) {
                        let _ = writeln!(buf, "{:i$}{label}", "", i = stack.len() * 2);
                    }
                }
            },
            WalkEvent::Leave(element) => {
                if let NodeOrToken::Node(node) = element {
                    if stack.last() == Some(&node) {
                        stack.pop();
                    }
                }
            }
        }
    }
    buf
}

fn describe_node(node: &SyntaxNode, lines: &LineIndex) -> Option<String> {
    let kind = format!("{:?}", node.kind());
    let at = span(node, lines);

    let label = match kind.as_str() {
        "SOURCE_FILE" => "FILE".to_string(),
        "MODULE" => format!("mod {}{at}", name_of(node)),
        "IMPL" => format!("{}{at}", header(node)),
        "TRAIT" => format!("trait {}{at}", name_of(node)),
        "FN" => format!("fn {}{}{}{at}", modifiers(node), owner_prefix(node), name_of(node)),
        "STRUCT" => format!("struct {}{at}", name_of(node)),
        "ENUM" => format!("enum {}{at}", name_of(node)),
        "UNION" => format!("union {}{at}", name_of(node)),
        "TYPE_ALIAS" => format!("type {}{at}", name_of(node)),
        "CONST" => format!("const {}{at}", name_of(node)),
        "STATIC" => format!("static {}{at}", name_of(node)),
        "MACRO_RULES" | "MACRO_DEF" => format!("macro {}{at}", name_of(node)),
        "VARIANT" => format!("variant {}{at}", name_of(node)),
        "RECORD_FIELD" | "TUPLE_FIELD" => {
            format!("field {} : {}{at}", name_of(node), field_type(node))
        }
        "CALL_EXPR" => format!("CALL -> {}()", callee(node)),
        // The receiver is the only thing that distinguishes `self.map.get(..)`
        // from a recursive `self.get(..)`, so never print a method call without it.
        "METHOD_CALL_EXPR" => format!(
            "METHOD CALL -> {}.{}()",
            node.children()
                .next()
                .map(|c| squish(&c.text().to_string()))
                .unwrap_or_else(|| "?".into()),
            child_text(node, "NAME_REF")
        ),
        "MACRO_CALL" => format!("MACRO CALL -> {}!", child_text(node, "PATH")),
        "CLOSURE_EXPR" => format!("closure{at}"),
        "ATTR" => format!("attr {}", squish(&node.text().to_string())),
        _ => return None,
    };
    Some(label)
}

fn describe_token(token: &SyntaxToken, lines: &LineIndex) -> Option<String> {
    if format!("{:?}", token.kind()) != "COMMENT" {
        return None;
    }
    let text = token.text();
    let is_doc = text.starts_with("///") || text.starts_with("//!") || text.starts_with("/**");
    let parent = token
        .parent()
        .map(|p| format!("{:?}", p.kind()))
        .unwrap_or_else(|| "?".into());
    let start: u32 = token.text_range().start().into();
    Some(format!(
        "{} [parent {parent}] L{} {:?}",
        if is_doc { "DOC COMMENT" } else { "comment" },
        lines.line(start as usize),
        squish(text),
    ))
}

fn span(node: &SyntaxNode, lines: &LineIndex) -> String {
    let r = node.text_range();
    let s: u32 = r.start().into();
    let e: u32 = r.end().into();
    let (a, b) = (lines.line(s as usize), lines.line(e as usize));
    if a == b {
        format!("   L{a}")
    } else {
        format!("   L{a}-{b}")
    }
}

/// Text of the first direct child node of the given kind.
fn child_text(node: &SyntaxNode, kind: &str) -> String {
    node.children()
        .find(|c| format!("{:?}", c.kind()) == kind)
        .map(|c| squish(&c.text().to_string()))
        .unwrap_or_else(|| "?".into())
}

fn name_of(node: &SyntaxNode) -> String {
    node.children()
        .find(|c| format!("{:?}", c.kind()) == "NAME")
        .map(|c| c.text().to_string())
        .unwrap_or_else(|| "<unnamed>".into())
}

/// Keyword tokens and visibility appearing before the name, e.g. `pub async `.
fn modifiers(node: &SyntaxNode) -> String {
    let mut parts = Vec::new();
    for child in node.children_with_tokens() {
        let kind = format!("{:?}", child.kind());
        if kind == "NAME" {
            break;
        }
        match &child {
            NodeOrToken::Node(n) if kind == "VISIBILITY" => parts.push(n.text().to_string()),
            NodeOrToken::Token(t) if kind.ends_with("_KW") && kind != "FN_KW" => {
                parts.push(t.text().to_string())
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{} ", parts.join(" "))
    }
}

/// Qualifier that makes a function's identity unique: `Cache::`, `Store::`, or
/// `<Cache as Store>::` for a trait implementation. Empty for a free function.
fn owner_prefix(fn_node: &SyntaxNode) -> String {
    for ancestor in fn_node.ancestors().skip(1) {
        match format!("{:?}", ancestor.kind()).as_str() {
            "IMPL" => {
                let mut before = None;
                let mut after = None;
                let mut seen_for = false;
                for c in ancestor.children_with_tokens() {
                    let k = format!("{:?}", c.kind());
                    if k == "FOR_KW" {
                        seen_for = true;
                        continue;
                    }
                    if let NodeOrToken::Node(n) = &c {
                        if k.ends_with("TYPE") {
                            let name = squish(&n.text().to_string());
                            if seen_for {
                                after.get_or_insert(name);
                            } else {
                                before.get_or_insert(name);
                            }
                        }
                    }
                }
                return match (before, after) {
                    (Some(tr), Some(ty)) => format!("<{ty} as {tr}>::"),
                    (Some(ty), None) => format!("{ty}::"),
                    (None, Some(ty)) => format!("{ty}::"),
                    (None, None) => String::new(),
                };
            }
            "TRAIT" => return format!("{}::", name_of(&ancestor)),
            "FN" => return String::new(),
            _ => {}
        }
    }
    String::new()
}

/// The declaration head, up to the opening brace (used for `impl` blocks).
fn header(node: &SyntaxNode) -> String {
    let text = node.text().to_string();
    let head = text.split('{').next().unwrap_or(&text);
    squish(head)
}

fn field_type(node: &SyntaxNode) -> String {
    node.children()
        .find(|c| format!("{:?}", c.kind()).ends_with("TYPE"))
        .map(|c| squish(&c.text().to_string()))
        .unwrap_or_else(|| "?".into())
}

fn callee(node: &SyntaxNode) -> String {
    node.children()
        .next()
        .map(|c| squish(&c.text().to_string()))
        .unwrap_or_else(|| "?".into())
}

/// Collapse whitespace and truncate, so one entry stays on one line.
fn squish(s: &str) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() > 60 {
        format!("{}…", joined.chars().take(60).collect::<String>())
    } else {
        joined
    }
}
