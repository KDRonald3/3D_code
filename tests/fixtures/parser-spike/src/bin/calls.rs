//! Spike 2: within-file and cross-file call resolution over a directory of Rust.
//!
//! Pass 1 parses each file alone and records local facts: the functions it
//! defines (with a path identity), the types it declares and their field types,
//! the `let` bindings inside each function, and every call site as an
//! *unresolved* name tagged with its enclosing function.
//!
//! Pass 2 indexes those facts project-wide and resolves each call. The guiding
//! rule is that a wrong edge is worse than no edge, so a call only resolves
//! when something concrete justifies it:
//!
//!   * the receiver's type is known -- from `self`, from a field's declared
//!     type, or from a `let` binding's initialiser (one hop, no inference)
//!   * the path is qualified by a type or module we actually have
//!   * the name is unique among candidates of the right syntactic form
//!
//! Anything else stays unresolved. Calls into std or third-party crates are
//! expected to be unresolved -- we never indexed them.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ra_ap_syntax::ast::HasName;
use ra_ap_syntax::{ast, AstNode, Edition, NodeOrToken, SourceFile, SyntaxNode};

// ── Facts ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Def {
    /// `cache.rs::<Cache as Store>::get`
    id: String,
    name: String,
    /// Type for an inherent method, trait for a trait declaration, else none.
    owner: Option<String>,
    /// Set when this is a trait implementation, so it can't collide with the
    /// inherent method of the same name on the same type.
    via_trait: Option<String>,
    file: usize,
    line: usize,
    is_method: bool,
    is_pub: bool,
    is_test: bool,
    has_body: bool,
    /// Variable name -> type name, for receivers we can type in one hop.
    locals: HashMap<String, String>,
    /// Every name bound inside this function: parameters, `let`s, closure
    /// parameters. Calling one of these is an indirect call through a value,
    /// so no amount of name lookup will ever resolve it.
    bindings: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallKind {
    Path,
    Method,
    Macro,
}

#[derive(Debug, Clone)]
struct CallSite {
    caller: Option<usize>,
    file: usize,
    line: usize,
    kind: CallKind,
    name: String,
    /// `Cache` in `Cache::new()`, or `lang` in `lang::idents()`.
    qualifier: Option<String>,
    /// Receiver source text for a method call, e.g. `self`, `self.map`, `cache`.
    receiver: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Confidence {
    Guess,
    Likely,
    Certain,
}

struct Outcome {
    def: usize,
    confidence: Confidence,
    rule: &'static str,
}

/// Why a call produced no edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Skip {
    /// Receiver or qualifier is a type we never indexed -- std or a dependency.
    ExternalType,
    /// No definition anywhere has this name in a compatible form.
    UnknownName,
    /// Several equally plausible targets and nothing to choose between them.
    Ambiguous,
    /// Macro invocation; macro definitions aren't indexed.
    Macro,
    /// Callee is a local binding -- a parameter, a `let`, a closure. The target
    /// is a value chosen at run time, not a name we could look up.
    Indirect,
}

/// What a `use` declaration says a local name refers to. `use a::b::c as d`
/// records local `d` -> target `c` with prefix `["a", "b"]`.
#[derive(Debug, Clone)]
struct ImportTarget {
    /// The real name of the item, i.e. the final path segment.
    name: String,
    /// The segments before it, outermost first.
    prefix: Vec<String>,
    /// True when the `use` was not at file top level, so treating it as
    /// file-wide over-approximates its real scope.
    scoped: bool,
}

/// Every name a file's `use` declarations bring into scope.
#[derive(Debug, Clone, Default)]
struct FileImports {
    /// Local name -> where it came from.
    named: HashMap<String, ImportTarget>,
    /// Module prefixes from `use path::*`, which bind an unknown set of names.
    globs: Vec<Vec<String>>,
}

/// Where an imported path lands.
enum ImportSite {
    /// A file we indexed.
    File(usize),
    /// Rooted in this crate, but through a module we couldn't map to a file.
    InProject,
    /// std or a dependency. Nothing we could ever link to, so an edge to a
    /// same-named project function would be wrong.
    External,
    /// A root we can't classify. Not confident enough to refuse the call, so
    /// fall back to the ordinary heuristics rather than suppressing an edge.
    Unknown,
}

struct Project {
    files: Vec<String>,
    defs: Vec<Def>,
    calls: Vec<CallSite>,
    by_name: HashMap<String, Vec<usize>>,
    /// Type name -> field name -> field's type name.
    fields: HashMap<String, HashMap<String, String>>,
    /// Every type name declared anywhere in the project.
    types: HashSet<String>,
    /// File stem and inline module name -> file index, for module-qualified paths.
    modules: HashMap<String, usize>,
    /// The crate root, so `crate::foo()` has somewhere to look.
    root_file: Option<usize>,
    /// This crate's own package name, which its binaries use to import from it.
    crate_name: Option<String>,
    /// File index -> the names its `use` declarations bring into scope.
    imports: HashMap<usize, FileImports>,
    /// `use` declarations found inside a function or an inline module. We
    /// attribute them to the whole file, which is wider than their real scope.
    nested_uses: usize,
    /// Call-shaped token sequences sitting inside macro arguments, which the
    /// parser hands back as unstructured token trees. These are real calls we
    /// cannot see; counted so the blind spot is visible rather than silent.
    hidden_in_macros: usize,
}

fn main() {
    let root = PathBuf::from(
        std::env::args().nth(1).unwrap_or_else(|| r"C:\Users\kouat\code\Horizon\src".into()),
    );

    let mut files: Vec<String> = Vec::new();
    collect_rs(&root, &root, &mut files);
    files.sort();
    if files.is_empty() {
        eprintln!("no .rs files under {}", root.display());
        return;
    }

    let started = std::time::Instant::now();
    let mut project = Project {
        files,
        defs: Vec::new(),
        calls: Vec::new(),
        by_name: HashMap::new(),
        fields: HashMap::new(),
        types: HashSet::new(),
        modules: HashMap::new(),
        root_file: None,
        crate_name: crate_name_of(&root),
        imports: HashMap::new(),
        nested_uses: 0,
        hidden_in_macros: 0,
    };
    project.root_file =
        project.files.iter().position(|f| f == "lib.rs" || f == "main.rs");
    let mut total_bytes = 0usize;

    for fi in 0..project.files.len() {
        let rel = project.files[fi].clone();
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else { continue };
        total_bytes += text.len();
        let parse = SourceFile::parse(&text, Edition::CURRENT);
        extract(&parse.syntax_node(), fi, &rel, &LineIndex::new(&text), &mut project);
    }
    let extract_time = started.elapsed();

    let started = std::time::Instant::now();
    for (i, d) in project.defs.iter().enumerate() {
        project.by_name.entry(d.name.clone()).or_default().push(i);
    }
    if std::env::var("SPIKE_IMPORTS").is_ok() {
        let mut fis: Vec<&usize> = project.imports.keys().collect();
        fis.sort();
        for fi in fis {
            let t = &project.imports[fi];
            if t.named.is_empty() && t.globs.is_empty() {
                continue;
            }
            eprintln!("{}", project.files[*fi]);
            let mut names: Vec<_> = t.named.iter().collect();
            names.sort_by_key(|(k, _)| k.to_string());
            for (local, tgt) in names {
                let path = if tgt.prefix.is_empty() {
                    tgt.name.clone()
                } else {
                    format!("{}::{}", tgt.prefix.join("::"), tgt.name)
                };
                let scope = if tgt.scoped { "  [inner scope]" } else { "" };
                let alias = if *local == tgt.name { String::new() } else { format!("  (as {local})") };
                eprintln!("    {path}{alias}{scope}");
            }
            for g in &t.globs {
                eprintln!("    {}::*", g.join("::"));
            }
        }
    }
    if std::env::var("SPIKE_DEBUG").is_ok() {
        let mut keys: Vec<&String> = project.modules.keys().collect();
        keys.sort();
        eprintln!("modules: {keys:?}");
        for c in project.calls.iter().filter(|c| c.qualifier.as_deref() == Some("lang")) {
            let r = resolve(c, &project);
            let shown = match &r {
                Ok(o) => format!("OK {} [{}]", project.defs[o.def].id, o.rule),
                Err(e) => format!("ERR {e:?}"),
            };
            eprintln!(
                "  L{} {}::{} types_has_lang={} modules_has_lang={:?} -> {shown}",
                c.line,
                c.qualifier.as_deref().unwrap_or(""),
                c.name,
                project.types.contains("lang"),
                project.modules.get("lang"),
            );
        }
    }

    let results: Vec<Result<Outcome, Skip>> =
        project.calls.iter().map(|c| resolve(c, &project)).collect();
    let resolve_time = started.elapsed();

    report(&root, &project, &results, total_bytes, extract_time, resolve_time);
}

// ── Pass 1: per-file extraction ────────────────────────────────────────────

fn extract(root: &SyntaxNode, file: usize, rel: &str, lines: &LineIndex, p: &mut Project) {
    // Types and their field types, so `self.map.get(..)` can be typed later.
    for node in root.descendants() {
        let k = kind(&node);
        if !matches!(k.as_str(), "STRUCT" | "ENUM" | "UNION" | "TRAIT" | "TYPE_ALIAS") {
            continue;
        }
        let Some(name) = name_of(&node) else { continue };
        p.types.insert(name.clone());
        if k == "STRUCT" {
            let mut fields = HashMap::new();
            for f in node.descendants().filter(|d| kind(d) == "RECORD_FIELD") {
                if let (Some(fname), Some(ftype)) = (name_of(&f), type_of(&f)) {
                    fields.insert(fname, ftype);
                }
            }
            p.fields.insert(name, fields);
        }
    }

    // A file is addressable as a module by its stem; inline `mod`s too.
    if let Some(stem) = Path::new(rel).file_stem().map(|s| s.to_string_lossy().to_string()) {
        p.modules.insert(stem, file);
    }
    // Only an *inline* `mod foo { .. }` lives in this file. A bare `mod foo;`
    // is a declaration whose contents are in `foo.rs`, already mapped by stem.
    for m in root.descendants().filter(|d| kind(d) == "MODULE") {
        if child(&m, "ITEM_LIST").is_none() {
            continue;
        }
        if let Some(name) = name_of(&m) {
            p.modules.insert(name, file);
        }
    }

    // `use` declarations. A single one can bind many names, so walk the tree.
    {
        let table = p.imports.entry(file).or_default();
        for node in root.descendants().filter(|d| kind(d) == "USE") {
            let Some(use_item) = ast::Use::cast(node.clone()) else { continue };
            let Some(tree) = use_item.use_tree() else { continue };
            // Top level means a direct child of the file. Anything else -- inside
            // a function or an inline `mod` -- has a narrower real scope.
            let top_level = node.parent().map(|par| kind(&par) == "SOURCE_FILE").unwrap_or(false);
            if !top_level {
                p.nested_uses += 1;
            }
            walk_use_tree(&tree, &[], !top_level, table);
        }
    }

    // Functions.
    let mut fn_to_def: HashMap<SyntaxNode, usize> = HashMap::new();
    for node in root.descendants().filter(|d| kind(d) == "FN") {
        let name = name_of(&node).unwrap_or_else(|| "<unnamed>".into());
        let (owner, via_trait) = ownership(&node);
        let is_method = owner.is_some();
        // Inline modules and enclosing functions are part of the identity. Two
        // `mod`s in one file may each define `snake`, and two functions may each
        // nest an `inner`; flattening them to `file::name` would collide.
        let containers = containers(&node);
        let scope =
            if containers.is_empty() { String::new() } else { format!("{}::", containers.join("::")) };
        let id = match (&owner, &via_trait) {
            (Some(o), Some(t)) => format!("{rel}::{scope}<{o} as {t}>::{name}"),
            (Some(o), None) => format!("{rel}::{scope}{o}::{name}"),
            (None, _) => format!("{rel}::{scope}{name}"),
        };
        p.defs.push(Def {
            id,
            name,
            owner,
            via_trait,
            file,
            line: lines.line(start_of(&node)),
            is_method,
            is_pub: child(&node, "VISIBILITY").is_some(),
            is_test: has_attr(&node, "test"),
            has_body: child(&node, "BLOCK_EXPR").is_some(),
            locals: HashMap::new(),
            bindings: HashSet::new(),
        });
        fn_to_def.insert(node, p.defs.len() - 1);
    }

    // Every name bound in a function body or signature. A call to one of these
    // is a call through a value, which is a different thing from an unknown name.
    for pat in root.descendants().filter(|d| kind(d) == "IDENT_PAT") {
        let parent_kind = pat.parent().map(|p| kind(&p)).unwrap_or_default();
        if !matches!(parent_kind.as_str(), "PARAM" | "LET_STMT") {
            continue;
        }
        let Some(di) = enclosing_fn(&pat).and_then(|f| fn_to_def.get(&f).copied()) else { continue };
        if let Some(n) = name_of(&pat) {
            p.defs[di].bindings.insert(n);
        }
    }

    // Calls buried in macro arguments, which we can count but not read.
    for tt in root.descendants().filter(|d| kind(d) == "TOKEN_TREE") {
        let elems: Vec<_> = tt.children_with_tokens().collect();
        for w in elems.windows(2) {
            let ident = matches!(&w[0], NodeOrToken::Token(t) if format!("{:?}", t.kind()) == "IDENT");
            let args = match &w[1] {
                NodeOrToken::Node(n) => {
                    kind(n) == "TOKEN_TREE"
                        && n.first_token()
                            .map(|t| format!("{:?}", t.kind()) == "L_PAREN")
                            .unwrap_or(false)
                }
                _ => false,
            };
            if ident && args {
                p.hidden_in_macros += 1;
            }
        }
    }

    // `let` bindings, attributed to their enclosing function.
    for stmt in root.descendants().filter(|d| kind(d) == "LET_STMT") {
        let Some(di) = enclosing_fn(&stmt).and_then(|f| fn_to_def.get(&f).copied()) else {
            continue;
        };
        let Some(var) = child(&stmt, "IDENT_PAT").and_then(|pat| name_of(&pat)) else { continue };
        if let Some(ty) = let_type(&stmt) {
            p.defs[di].locals.insert(var, ty);
        }
    }

    // Call sites.
    for node in root.descendants() {
        let Some((kind_, name, qualifier, receiver)) = call_shape(&node) else { continue };
        let caller = enclosing_fn(&node).and_then(|f| fn_to_def.get(&f).copied());
        p.calls.push(CallSite {
            caller,
            file,
            line: lines.line(start_of(&node)),
            kind: kind_,
            name,
            qualifier,
            receiver,
        });
    }
}

/// Walk a `use` tree, accumulating the path prefix on the way down.
///
/// The grammar is `UseTree = (Path? '::')? ('*' | UseTreeList) | Path Rename?`
/// with `UseTreeList` mutually recursive with `UseTree`, so nesting is
/// unbounded and one declaration can bind any number of names.
fn walk_use_tree(tree: &ast::UseTree, prefix: &[String], scoped: bool, out: &mut FileImports) {
    let mut here: Vec<String> = prefix.to_vec();
    if let Some(path) = tree.path() {
        here.extend(path_segments(&path));
    }

    if tree.star_token().is_some() {
        out.globs.push(here);
        return;
    }

    if let Some(list) = tree.use_tree_list() {
        for child in list.use_trees() {
            walk_use_tree(&child, &here, scoped, out);
        }
        return;
    }

    // `use a::b::{self}` binds `b`, not a name called "self".
    if here.last().map(|s| s == "self").unwrap_or(false) {
        here.pop();
    }
    let Some(name) = here.last().cloned() else { return };
    let target =
        ImportTarget { name, prefix: here[..here.len() - 1].to_vec(), scoped };

    match tree.rename() {
        // `use Trait as _;` brings a trait into scope for method resolution but
        // binds no name a call site could ever write.
        Some(r) if r.underscore_token().is_some() => {}
        Some(r) => {
            if let Some(local) = r.name() {
                out.named.insert(local.syntax().text().to_string(), target);
            }
        }
        None => {
            out.named.insert(target.name.clone(), target);
        }
    }
}

/// Path segments outermost first. The last segment is the shallowest node and
/// the crate root the deepest, so walk the qualifier chain and reverse rather
/// than recursing to a fixed depth.
fn path_segments(path: &ast::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = Some(path.clone());
    while let Some(p) = cursor {
        if let Some(seg) = p.segment() {
            out.push(segment_text(&seg));
        }
        cursor = p.qualifier();
    }
    out.reverse();
    out
}

/// `crate`, `self` and `super` sit inside a `NameRef` just as an identifier
/// does, so the name reference covers them; the token accessors are a fallback.
fn segment_text(seg: &ast::PathSegment) -> String {
    if let Some(nr) = seg.name_ref() {
        return nr.syntax().text().to_string();
    }
    if seg.crate_token().is_some() {
        return "crate".into();
    }
    if seg.self_token().is_some() {
        return "self".into();
    }
    if seg.super_token().is_some() {
        return "super".into();
    }
    String::new()
}

/// Classify a node as a call, returning its kind, callee name, qualifier and receiver.
fn call_shape(node: &SyntaxNode) -> Option<(CallKind, String, Option<String>, Option<String>)> {
    match kind(node).as_str() {
        "CALL_EXPR" => {
            let callee = node.children().next()?;
            let path = squish(&callee.text().to_string());
            let mut segs: Vec<&str> = path.split("::").collect();
            let name = segs.pop()?.to_string();
            if !is_ident(&name) {
                return None; // calling a closure, a field, or an expression
            }
            let qualifier = segs.pop().map(|s| strip_generics(s));
            Some((CallKind::Path, name, qualifier, None))
        }
        "METHOD_CALL_EXPR" => {
            let name = child(node, "NAME_REF")?.text().to_string();
            let receiver = node.children().next().map(|c| squish(&c.text().to_string()));
            Some((CallKind::Method, name, None, receiver))
        }
        "MACRO_CALL" => {
            let path = squish(&child(node, "PATH")?.text().to_string());
            Some((CallKind::Macro, path.rsplit("::").next()?.to_string(), None, None))
        }
        _ => None,
    }
}

/// Inline modules and enclosing functions wrapping this item, outermost first.
fn containers(node: &SyntaxNode) -> Vec<String> {
    let mut parts: Vec<String> = node
        .ancestors()
        .skip(1)
        .filter(|a| matches!(kind(a).as_str(), "MODULE" | "FN"))
        .filter_map(|a| name_of(&a))
        .collect();
    parts.reverse();
    parts
}

/// `(owner, via_trait)` for a function: the type it belongs to, and the trait it
/// implements if it came from an `impl Trait for Type` block.
fn ownership(fn_node: &SyntaxNode) -> (Option<String>, Option<String>) {
    for ancestor in fn_node.ancestors().skip(1) {
        match kind(&ancestor).as_str() {
            "IMPL" => return impl_parts(&ancestor),
            "TRAIT" => return (name_of(&ancestor), None),
            "FN" => return (None, None), // nested function: a free function in scope
            _ => {}
        }
    }
    (None, None)
}

/// For `impl Cache` -> (`Cache`, None); for `impl Store for Cache` -> (`Cache`, `Store`).
fn impl_parts(impl_node: &SyntaxNode) -> (Option<String>, Option<String>) {
    let mut before: Option<String> = None;
    let mut after: Option<String> = None;
    let mut seen_for = false;
    for c in impl_node.children_with_tokens() {
        let k = format!("{:?}", c.kind());
        if k == "FOR_KW" {
            seen_for = true;
            continue;
        }
        if let NodeOrToken::Node(n) = &c {
            if k.ends_with("TYPE") {
                let name = strip_generics(&squish(&n.text().to_string()));
                if seen_for {
                    after.get_or_insert(name);
                } else {
                    before.get_or_insert(name);
                }
            }
        }
    }
    match (before, after) {
        (Some(tr), Some(ty)) => (Some(ty), Some(tr)),
        (Some(ty), None) => (Some(ty), None),
        (None, ty) => (ty, None),
    }
}

/// Type of a `let` binding: explicit annotation, `Type::f()` initialiser, or
/// a `Type { .. }` struct literal.
fn let_type(stmt: &SyntaxNode) -> Option<String> {
    if let Some(t) = stmt.children().find(|c| kind(c).ends_with("TYPE")) {
        return Some(strip_generics(&squish(&t.text().to_string())));
    }
    let init = stmt.children().find(|c| {
        matches!(kind(c).as_str(), "CALL_EXPR" | "RECORD_EXPR" | "METHOD_CALL_EXPR" | "PATH_EXPR")
    })?;
    match kind(&init).as_str() {
        "CALL_EXPR" => {
            let path = squish(&init.children().next()?.text().to_string());
            let mut segs: Vec<&str> = path.split("::").collect();
            segs.pop();
            segs.pop().map(strip_generics)
        }
        "RECORD_EXPR" => Some(strip_generics(&squish(&child(&init, "PATH")?.text().to_string()))),
        _ => None,
    }
}

// ── Pass 2: resolution ─────────────────────────────────────────────────────

fn resolve(call: &CallSite, p: &Project) -> Result<Outcome, Skip> {
    if call.kind == CallKind::Macro {
        return Err(Skip::Macro);
    }

    let ok = |def: usize, confidence: Confidence, rule: &'static str| {
        Ok(Outcome { def, confidence, rule })
    };

    // A call's syntactic form limits what it can bind to. This is what keeps
    // `.collect()` away from a free function named `collect`.
    let compatible = |d: usize| -> bool {
        let def = &p.defs[d];
        match call.kind {
            CallKind::Method => def.is_method,
            CallKind::Path if call.qualifier.is_some() => def.is_method || !def.is_method,
            CallKind::Path => !def.is_method,
            CallKind::Macro => false,
        }
    };
    let named: Vec<usize> = p
        .by_name
        .get(&call.name)
        .map(|v| v.iter().copied().filter(|&d| compatible(d)).collect())
        .unwrap_or_default();

    // ── Method calls: try to type the receiver in one hop ──
    if call.kind == CallKind::Method {
        if let Some(recv) = &call.receiver {
            match receiver_type(recv, call, p) {
                // Receiver is a project type: only its own methods can match.
                ReceiverType::Project(ty) => {
                    let hit = named.iter().copied().find(|&d| {
                        p.defs[d].owner.as_deref() == Some(ty.as_str())
                            && p.defs[d].via_trait.is_none()
                    });
                    if let Some(d) = hit {
                        return ok(d, Confidence::Certain, "receiver type");
                    }
                    // Could be a trait impl on that type, or an inherited method.
                    let via = named
                        .iter()
                        .copied()
                        .find(|&d| p.defs[d].owner.as_deref() == Some(ty.as_str()));
                    if let Some(d) = via {
                        return ok(d, Confidence::Certain, "receiver type (trait impl)");
                    }
                    return Err(Skip::ExternalType);
                }
                // Receiver is std / a dependency: refuse rather than guess.
                ReceiverType::Foreign => return Err(Skip::ExternalType),
                ReceiverType::Unknown => {}
            }
        }
        // Receiver untyped. Only a project-wide unique method name is defensible;
        // "unique in this file" means nothing for a method.
        if named.is_empty() {
            return Err(Skip::UnknownName);
        }
        let owners: HashSet<&str> =
            named.iter().filter_map(|&d| p.defs[d].owner.as_deref()).collect();
        if owners.len() == 1 && named.len() == 1 {
            return ok(named[0], Confidence::Likely, "unique method name");
        }
        return Err(Skip::Ambiguous);
    }

    // ── Qualified paths: the qualifier must be something we actually have ──
    if let Some(qual) = &call.qualifier {
        // `crate`, `self` and `super` name a scope, not a module. Looking for a
        // module called "crate" was making every `crate::foo()` look external.
        let in_file = |ds: &[usize]| ds.iter().copied().find(|&d| p.defs[d].file == call.file);
        let in_root =
            |ds: &[usize]| p.root_file.and_then(|rf| ds.iter().copied().find(|&d| p.defs[d].file == rf));
        match qual.as_str() {
            "crate" => {
                if let Some(d) = in_root(&named) {
                    return ok(d, Confidence::Certain, "crate root");
                }
                // Not defined in the root, so the root is re-exporting it.
                if let Some(rf) = p.root_file {
                    if let Ok(o) = follow_import(&call.name, rf, p, &compatible, 1) {
                        return Ok(o);
                    }
                }
                if named.len() == 1 {
                    return ok(named[0], Confidence::Likely, "crate root re-export");
                }
                return Err(if named.is_empty() { Skip::UnknownName } else { Skip::Ambiguous });
            }
            "self" => {
                return match in_file(&named) {
                    Some(d) => ok(d, Confidence::Certain, "self module"),
                    None => Err(Skip::UnknownName),
                };
            }
            // Exact when the caller sits in an inline `mod`, since the parent is
            // then the same file. Approximate when it isn't.
            "super" => {
                if let Some(d) = in_file(&named) {
                    return ok(d, Confidence::Certain, "super module");
                }
                if let Some(d) = in_root(&named) {
                    return ok(d, Confidence::Likely, "super module (root)");
                }
                return Err(Skip::UnknownName);
            }
            _ => {}
        }
        if p.types.contains(qual) {
            let hit = named.iter().copied().find(|&d| p.defs[d].owner.as_deref() == Some(qual));
            return match hit {
                Some(d) => ok(d, Confidence::Certain, "qualified by type"),
                None => Err(Skip::UnknownName),
            };
        }
        if let Some(&fi) = p.modules.get(qual) {
            if let Some(d) = named.iter().copied().find(|&d| p.defs[d].file == fi) {
                return ok(d, Confidence::Certain, "qualified by module");
            }
            // Ours, but the name isn't declared there, so it is re-exported.
            if let Ok(o) = follow_import(&call.name, fi, p, &compatible, 1) {
                return Ok(o);
            }
            return Err(Skip::UnknownName);
        }
        // `Vec::new`, `String::from`, `std::ptr::null` -- not ours.
        return Err(Skip::ExternalType);
    }

    // ── Bare function calls: Rust scoping makes same-file the right first look ──
    // A local binding shadows any function of the same name, and the target is
    // whatever value it holds, so this is unresolvable in principle -- not merely
    // a name we failed to find.
    if let Some(ci) = call.caller {
        if p.defs[ci].bindings.contains(&call.name) {
            return Err(Skip::Indirect);
        }
    }
    // Rust looks in the current module first, and an item declared here beats
    // anything a `use` brought in -- a glob import especially.
    let in_file: Vec<usize> = named.iter().copied().filter(|&d| p.defs[d].file == call.file).collect();
    if in_file.len() == 1 {
        return ok(in_file[0], Confidence::Certain, "unique in file");
    }
    // Then whatever the file's `use` declarations state. This is read off the
    // source rather than inferred from a name happening to be unique.
    match follow_import(&call.name, call.file, p, &compatible, 0) {
        Ok(o) => return Ok(o),
        Err(Skip::ExternalType) => return Err(Skip::ExternalType),
        Err(_) => {}
    }
    if named.is_empty() {
        return Err(Skip::UnknownName);
    }
    if named.len() == 1 {
        return ok(named[0], Confidence::Likely, "unique in project");
    }
    // Ambiguous: nearest in the directory tree, reported honestly as a guess.
    let best = *named
        .iter()
        .max_by_key(|&&d| {
            let same = (p.defs[d].file == call.file) as usize * 1000;
            same + shared_prefix(&p.files[call.file], &p.files[p.defs[d].file])
        })
        .unwrap();
    ok(best, Confidence::Guess, "ambiguous, nearest")
}

/// Resolve a name that a file's `use` declarations bring into scope, following
/// `pub use` re-export chains as far as they go.
fn follow_import(
    name: &str,
    file: usize,
    p: &Project,
    compat: &dyn Fn(usize) -> bool,
    depth: u32,
) -> Result<Outcome, Skip> {
    if depth > 4 {
        return Err(Skip::UnknownName);
    }
    let candidates = |n: &str| -> Vec<usize> {
        p.by_name
            .get(n)
            .map(|v| v.iter().copied().filter(|&d| compat(d)).collect())
            .unwrap_or_default()
    };

    // Declared in this very file. Only reachable while chasing a re-export --
    // at depth zero the caller has already checked its own file.
    if depth > 0 {
        if let Some(d) = candidates(name).into_iter().find(|&d| p.defs[d].file == file) {
            return Ok(Outcome {
                def: d,
                confidence: Confidence::Certain,
                rule: "imported (re-export)",
            });
        }
    }

    let Some(table) = p.imports.get(&file) else { return Err(Skip::UnknownName) };

    if let Some(target) = table.named.get(name) {
        let base = if target.scoped { Confidence::Likely } else { Confidence::Certain };
        let rule = if target.scoped {
            "imported (inner scope)"
        } else if depth > 0 {
            "imported (re-export)"
        } else {
            "imported"
        };
        let cands = candidates(&target.name);
        match import_site(target, file, p) {
            // The `use` names a dependency. Any same-named project function is
            // the wrong answer, so refuse instead of falling through to a guess.
            ImportSite::External => Err(Skip::ExternalType),
            ImportSite::File(fi) => {
                if let Some(d) = cands.iter().copied().find(|&d| p.defs[d].file == fi) {
                    return Ok(Outcome { def: d, confidence: base, rule });
                }
                // Our module, but it doesn't define the name, so it is itself
                // re-exporting from somewhere else.
                if let Ok(o) = follow_import(&target.name, fi, p, compat, depth + 1) {
                    return Ok(Outcome {
                        def: o.def,
                        confidence: base.min(o.confidence),
                        rule: "imported (re-export)",
                    });
                }
                if cands.len() == 1 {
                    return Ok(Outcome {
                        def: cands[0],
                        confidence: Confidence::Likely,
                        rule: "imported (unverified)",
                    });
                }
                Err(if cands.is_empty() { Skip::UnknownName } else { Skip::Ambiguous })
            }
            ImportSite::InProject => {
                if cands.len() == 1 {
                    return Ok(Outcome { def: cands[0], confidence: base, rule });
                }
                Err(if cands.is_empty() { Skip::UnknownName } else { Skip::Ambiguous })
            }
            // Unclassifiable root. If exactly one project function fits, the
            // import at least tells us the name was deliberately brought into
            // scope, but we can't verify where from.
            ImportSite::Unknown => {
                if cands.len() == 1 {
                    return Ok(Outcome {
                        def: cands[0],
                        confidence: Confidence::Likely,
                        rule: "imported (unmapped root)",
                    });
                }
                Err(Skip::UnknownName)
            }
        }
    } else {
        // A glob binds a set of names we can only learn by reading the target
        // module. Weaker than an explicit `use`, and already beaten by any local
        // definition, which is Rust's rule.
        for prefix in &table.globs {
            let probe =
                ImportTarget { name: name.to_string(), prefix: prefix.clone(), scoped: false };
            if let ImportSite::File(fi) = import_site(&probe, file, p) {
                if let Some(d) = candidates(name).into_iter().find(|&d| p.defs[d].file == fi) {
                    return Ok(Outcome {
                        def: d,
                        confidence: Confidence::Likely,
                        rule: "glob import",
                    });
                }
            }
        }
        Err(Skip::UnknownName)
    }
}

/// Where the module part of an imported path lands.
fn import_site(target: &ImportTarget, from_file: usize, p: &Project) -> ImportSite {
    let first = target.prefix.first().map(|s| s.as_str()).unwrap_or("");
    // Decide in-crate versus dependency from the ROOT of the path, so that a
    // project module sharing a name with a std one can't hijack `std::fmt::..`.
    match first {
        "crate" | "self" | "super" => {}
        // A crate referring to itself by package name, which is how a binary
        // in `src/bin/` reaches its own library. In-project despite looking
        // like a dependency.
        _ if Some(first) == p.crate_name.as_deref() => {
            return match p.root_file {
                Some(rf) => ImportSite::File(rf),
                None => ImportSite::InProject,
            };
        }
        _ if p.modules.contains_key(first) => {}
        "std" | "core" | "alloc" => return ImportSite::External,
        // `use foo;` with no prefix at all names a module or an extern crate,
        // not an item we could link to.
        "" => return ImportSite::External,
        // Some other crate root. Probably a dependency, but we have no index of
        // dependencies to confirm it, so don't let it veto an edge.
        _ => return ImportSite::Unknown,
    }

    let Some(last) = target.prefix.last() else { return ImportSite::External };
    match last.as_str() {
        "crate" => match p.root_file {
            Some(rf) => ImportSite::File(rf),
            None => ImportSite::InProject,
        },
        "self" => ImportSite::File(from_file),
        "super" => ImportSite::InProject,
        m => match p.modules.get(m) {
            Some(&fi) => ImportSite::File(fi),
            None => ImportSite::InProject,
        },
    }
}

enum ReceiverType {
    /// A type declared in this project.
    Project(String),
    /// A type from std or a dependency.
    Foreign,
    /// Couldn't tell.
    Unknown,
}

/// Type a receiver expression in at most two hops, with no inference:
/// `self`, `self.field`, `local`, `local.field`.
fn receiver_type(recv: &str, call: &CallSite, p: &Project) -> ReceiverType {
    let Some(ci) = call.caller else { return ReceiverType::Unknown };
    let caller = &p.defs[ci];

    let classify = |name: &str| -> ReceiverType {
        if p.types.contains(name) {
            ReceiverType::Project(name.to_string())
        } else {
            ReceiverType::Foreign
        }
    };

    let field_of = |ty: &str, field: &str| -> ReceiverType {
        match p.fields.get(ty).and_then(|f| f.get(field)) {
            Some(ft) => classify(ft),
            None => ReceiverType::Unknown,
        }
    };

    let parts: Vec<&str> = recv.split('.').collect();
    match parts.as_slice() {
        ["self"] => match &caller.owner {
            Some(o) => ReceiverType::Project(o.clone()),
            None => ReceiverType::Unknown,
        },
        ["self", field] => match &caller.owner {
            Some(o) => field_of(o, field),
            None => ReceiverType::Unknown,
        },
        [var] if is_ident(var) => match caller.locals.get(*var) {
            Some(ty) => classify(ty),
            None => ReceiverType::Unknown,
        },
        [var, field] if is_ident(var) => match caller.locals.get(*var) {
            Some(ty) => field_of(ty, field),
            None => ReceiverType::Unknown,
        },
        _ => ReceiverType::Unknown,
    }
}

// ── Reporting ──────────────────────────────────────────────────────────────

fn report(
    root: &Path,
    p: &Project,
    results: &[Result<Outcome, Skip>],
    total_bytes: usize,
    extract_time: std::time::Duration,
    resolve_time: std::time::Duration,
) {
    let mut intra: Vec<String> = Vec::new();
    let mut inter: Vec<String> = Vec::new();
    let mut by_conf: HashMap<Confidence, usize> = HashMap::new();
    let mut by_rule: HashMap<&str, usize> = HashMap::new();
    let mut by_skip: HashMap<Skip, usize> = HashMap::new();
    let mut unresolved_names: HashMap<&str, usize> = HashMap::new();

    for (ci, r) in results.iter().enumerate() {
        let call = &p.calls[ci];
        match r {
            Err(skip) => {
                *by_skip.entry(*skip).or_default() += 1;
                *unresolved_names.entry(call.name.as_str()).or_default() += 1;
            }
            Ok(o) => {
                *by_conf.entry(o.confidence).or_default() += 1;
                *by_rule.entry(o.rule).or_default() += 1;
                let from = call
                    .caller
                    .map(|c| p.defs[c].id.clone())
                    .unwrap_or_else(|| format!("{}::<top level>", p.files[call.file]));
                let def = &p.defs[o.def];
                let line = call.line;
                let edge = format!(
                    "{from} (L{line})  ->  {} (L{})   [{:?}, {}]",
                    def.id, def.line, o.confidence, o.rule
                );
                if def.file == call.file {
                    intra.push(edge);
                } else {
                    inter.push(edge);
                }
            }
        }
    }

    let resolved = intra.len() + inter.len();
    println!("root                  : {}", root.display());
    println!("files                 : {}", p.files.len());
    println!("source bytes          : {total_bytes}");
    println!(
        "extract (parse + walk): {:.1} ms  ({:.1} MB/s single core)",
        extract_time.as_secs_f64() * 1000.0,
        total_bytes as f64 / extract_time.as_secs_f64() / 1_000_000.0
    );
    println!("index + resolve       : {:.2} ms", resolve_time.as_secs_f64() * 1000.0);
    println!();
    println!("types declared        : {}", p.types.len());
    println!("function defs         : {}", p.defs.len());
    println!("  methods             : {}", p.defs.iter().filter(|d| d.is_method).count());
    println!(
        "  trait impls         : {}",
        p.defs.iter().filter(|d| d.via_trait.is_some()).count()
    );
    println!("  declarations only   : {}", p.defs.iter().filter(|d| !d.has_body).count());
    println!("  public              : {}", p.defs.iter().filter(|d| d.is_pub).count());
    println!("  tests               : {}", p.defs.iter().filter(|d| d.is_test).count());
    let imported_names: usize = p.imports.values().map(|t| t.named.len()).sum();
    let glob_count: usize = p.imports.values().map(|t| t.globs.len()).sum();
    println!("imported names        : {imported_names}  (glob imports: {glob_count})");
    if p.nested_uses > 0 {
        println!(
            "  `use` below file top: {}  (treated as file-wide, wider than their real scope)",
            p.nested_uses
        );
    }
    println!("call sites            : {}", p.calls.len());
    println!(
        "  path / method / macro: {} / {} / {}",
        p.calls.iter().filter(|c| c.kind == CallKind::Path).count(),
        p.calls.iter().filter(|c| c.kind == CallKind::Method).count(),
        p.calls.iter().filter(|c| c.kind == CallKind::Macro).count()
    );
    println!();
    println!(
        "RESOLVED              : {resolved} / {}  ({:.0}% of calls, {:.0}% of non-macro non-std)",
        p.calls.len(),
        resolved as f64 / p.calls.len() as f64 * 100.0,
        resolved as f64
            / (p.calls.len()
                - by_skip.get(&Skip::Macro).copied().unwrap_or(0)
                - by_skip.get(&Skip::ExternalType).copied().unwrap_or(0)
                - by_skip.get(&Skip::UnknownName).copied().unwrap_or(0))
            .max(1) as f64
            * 100.0
    );
    println!("  within-file edges   : {}", intra.len());
    println!("  cross-file edges    : {}", inter.len());
    println!();
    println!("confidence:");
    for c in [Confidence::Certain, Confidence::Likely, Confidence::Guess] {
        println!("  {:<9} {}", format!("{c:?}"), by_conf.get(&c).copied().unwrap_or(0));
    }
    println!("rules that fired:");
    let mut rules: Vec<_> = by_rule.iter().map(|(r, n)| (*r, *n)).collect();
    rules.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (rule, n) in &rules {
        println!("  {rule:<28} {n}");
    }
    let stated: usize = by_rule
        .iter()
        .filter(|(r, _)| r.starts_with("imported") || **r == "glob import")
        .map(|(_, n)| *n)
        .sum();
    let inferred: usize =
        by_rule.iter().filter(|(r, _)| r.contains("unique") || r.contains("ambiguous")).map(|(_, n)| *n).sum();
    println!("  ── stated by a `use`         {stated}");
    println!("  ── inferred from uniqueness  {inferred}");
    println!("not resolved, by reason:");
    for s in [Skip::ExternalType, Skip::UnknownName, Skip::Ambiguous, Skip::Indirect, Skip::Macro] {
        println!("  {:<14} {}", format!("{s:?}"), by_skip.get(&s).copied().unwrap_or(0));
    }
    println!("calls hidden in macro args (unparsed token trees): {}", p.hidden_in_macros);

    // Path calls and method calls fail for entirely different reasons, and one
    // combined percentage hides both.
    println!();
    println!("by call kind:");
    for (label, want) in [("path", CallKind::Path), ("method", CallKind::Method)] {
        let idx: Vec<usize> = (0..p.calls.len()).filter(|&i| p.calls[i].kind == want).collect();
        let done = idx.iter().filter(|&&i| results[i].is_ok()).count();
        let mut skips: HashMap<Skip, usize> = HashMap::new();
        for &i in &idx {
            if let Err(s) = &results[i] {
                *skips.entry(*s).or_default() += 1;
            }
        }
        let mut ss: Vec<_> = skips.into_iter().collect();
        ss.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        let detail: Vec<String> = ss.iter().map(|(s, n)| format!("{s:?} {n}")).collect();
        println!("  {label:<7} {done}/{} resolved   [{}]", idx.len(), detail.join(", "));
    }

    if std::env::var("SPIKE_METHODS").is_ok() {
        std::fs::create_dir_all("out").unwrap();
        let mut buf = String::new();
        for (ci, r) in results.iter().enumerate() {
            let c = &p.calls[ci];
            if c.kind != CallKind::Method {
                continue;
            }
            let from = c
                .caller
                .map(|d| p.defs[d].id.clone())
                .unwrap_or_else(|| format!("{}::<top>", p.files[c.file]));
            let outcome = match r {
                Ok(o) => format!("-> {}  [{:?}, {}]", p.defs[o.def].id, o.confidence, o.rule),
                Err(s) => format!("XX {s:?}"),
            };
            let _ = writeln!(
                buf,
                "{from} L{}   {}.{}()   {outcome}",
                c.line,
                c.receiver.clone().unwrap_or_default(),
                c.name
            );
        }
        std::fs::write(Path::new("out").join("methods.txt"), buf).unwrap();
        println!("wrote out/methods.txt");
    }

    println!();
    println!("── CROSS-FILE edges ({}) ──", inter.len());
    inter.sort();
    for e in inter.iter().take(30) {
        println!("  {e}");
    }
    println!();
    println!("── WITHIN-FILE edges ({}) ──", intra.len());
    intra.sort();
    for e in intra.iter().take(20) {
        println!("  {e}");
    }

    println!();
    println!("── most common unresolved names ──");
    let mut u: Vec<_> = unresolved_names.into_iter().collect();
    u.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (name, n) in u.iter().take(14) {
        print!("{name} x{n}   ");
    }
    println!();

    let out = Path::new("out");
    std::fs::create_dir_all(out).unwrap();
    let mut buf = String::new();
    let _ = writeln!(buf, "# CROSS-FILE ({})", inter.len());
    for e in &inter {
        let _ = writeln!(buf, "{e}");
    }
    let _ = writeln!(buf, "\n# WITHIN-FILE ({})", intra.len());
    for e in &intra {
        let _ = writeln!(buf, "{e}");
    }
    std::fs::write(out.join("edges.txt"), buf).unwrap();
    println!("\nwrote out/edges.txt");
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Guess the package name from the layout: for `<pkg>/src` it's `<pkg>`, with
/// the hyphen-to-underscore mapping cargo applies to crate names.
fn crate_name_of(root: &Path) -> Option<String> {
    let dir = root.file_name()?.to_string_lossy().to_string();
    let name = if dir == "src" {
        root.parent()?.file_name()?.to_string_lossy().to_string()
    } else {
        dir
    };
    Some(name.to_lowercase().replace('-', "_"))
}

fn collect_rs(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            if !matches!(name.as_str(), "target" | ".git" | "node_modules") {
                collect_rs(root, &path, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

fn kind(node: &SyntaxNode) -> String {
    format!("{:?}", node.kind())
}

fn child(node: &SyntaxNode, want: &str) -> Option<SyntaxNode> {
    node.children().find(|c| kind(c) == want)
}

fn name_of(node: &SyntaxNode) -> Option<String> {
    child(node, "NAME").map(|n| n.text().to_string())
}

fn type_of(node: &SyntaxNode) -> Option<String> {
    node.children()
        .find(|c| kind(c).ends_with("TYPE"))
        .map(|t| strip_generics(&squish(&t.text().to_string())))
}

fn enclosing_fn(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.ancestors().skip(1).find(|a| kind(a) == "FN")
}

fn has_attr(node: &SyntaxNode, want: &str) -> bool {
    node.children()
        .filter(|c| kind(c) == "ATTR")
        .any(|a| a.text().to_string().contains(want))
}

fn start_of(node: &SyntaxNode) -> usize {
    let s: u32 = node.text_range().start().into();
    s as usize
}

/// `HashMap<String,Item>` -> `HashMap`; `&[Item]` -> `Item`; `crate::a::B` -> `B`.
fn strip_generics(s: &str) -> String {
    let base = s.split('<').next().unwrap_or(s);
    let base = base.trim_start_matches(['&', '*']).trim_start_matches("mut ");
    let base = base.trim_matches(|c: char| matches!(c, '[' | ']' | '(' | ')' | ' '));
    base.rsplit("::").next().unwrap_or(base).to_string()
}

/// Number of leading path components two files share; higher means nearer.
fn shared_prefix(a: &str, b: &str) -> usize {
    let a: Vec<&str> = a.split(['/', '\\']).collect();
    let b: Vec<&str> = b.split(['/', '\\']).collect();
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn squish(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join("")
}

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
}
