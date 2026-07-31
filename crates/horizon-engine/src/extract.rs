//! Extract per-file facts from a syntax tree.
//!
//! Free function definitions, local type definitions (struct / enum / trait /
//! type alias), doc comments (`///` / `//!`), imports, and call sites —
//! including calls hidden inside **allowlisted** macro argument token trees.
//!
//! Macro recovery re-parses each allowlisted macro's token-tree interior as
//! ordinary Rust (arguments of a synthetic call, or array elements for
//! `[…]` macros) and walks the resulting `CallExpr` nodes. That prefers real
//! syntax over raw token scanning. Macros whose arguments are not expression
//! positions (`matches!`, `stringify!`, `cfg!`, `quote!`, …) are never
//! opened. `macro_rules!` / `macro` definition bodies are skipped entirely.
//! When recovery cannot tell whether a site is a real call, it omits it.
//!
//! Methods and `impl` items are out of scope.
//!
//! # Phase 3
//!
//! Builds the import table from `use` / `pub use` trees (plain paths, brace
//! lists, nested braces, aliases, `self` in a list, globs, relative
//! `crate`/`self`/`super` roots, and external roots), plus `extern crate`
//! / `pub extern crate … as alias` (edition-2015-style crate renames still
//! used as facades, e.g. ripgrep's `pub extern crate grep_cli as cli`).
//! Each leaf binding becomes one [`Import`]. Nested trees are walked via
//! the AST — not by string-splitting the written form.
//!
//! ## Function-body `use` scoping (known limitation)
//!
//! In Rust, a `use` inside a function body is scoped to that body. Extraction
//! still attributes such imports to the enclosing **module**
//! ([`Import::module_path`]). Resolution therefore treats them as module-wide.
//! That widening is a documented limitation, not a separately tracked flag:
//! narrowing body-scoped imports would change resolution outcomes and needs
//! its own design pass. Module-level `use` (including inside inline `mod`
//! blocks) is attributed to that module correctly.

use horizon_map::{DocComment, DocCommentKind, Function, FunctionId};
use anyhow::Result;
use ra_ap_syntax::ast::{
    self, AstNode, AstToken, HasName, HasVisibility, LiteralKind, PathSegmentKind, VisibilityKind,
};
use ra_ap_syntax::{SourceFile, SyntaxElement, SyntaxKind, SyntaxNode};
use std::collections::{HashMap, HashSet};

/// Facts extracted from one source file, before cross-file resolution.
#[derive(Debug, Clone, Default)]
pub struct FileFacts {
    pub functions: Vec<Function>,
    /// Visibility of each function in [`functions`] (same order), for glob
    /// candidate filtering. Not part of the emitted map JSON.
    pub function_visibility: Vec<ItemVisibility>,
    /// Local type definitions in this file (for constructor / assoc exclusion).
    pub types: Vec<TypeDef>,
    pub imports: Vec<Import>,
    /// Call sites not yet attached to a defining function or file.
    pub call_sites: Vec<PendingCall>,
    pub doc_comments: Vec<DocComment>,
}

/// Kind of type-level item collected during extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Struct,
    Enum,
    Trait,
    TypeAlias,
}

/// Visibility of an item, as written (`pub`, `pub(crate)`, …).
///
/// Used for glob candidate sets: a glob only brings in names that would be
/// visible through the globbed module from the importing module. Direct call
/// edges still do not filter on visibility (Phase 2 decision retained).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemVisibility {
    /// `pub`
    Public,
    /// `pub(crate)`
    Crate,
    /// `pub(super)`
    Super,
    /// `pub(self)`
    SelfMod,
    /// `pub(in path)` — path text preserved for best-effort checks.
    InPath(String),
    /// No `pub` — private to the defining module and its descendants.
    Private,
}

/// A type definition visible to the resolver for classifying non-function calls.
#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    /// Module path containing the type (e.g. `crate::map`), no type-name segment.
    pub module_path: String,
    pub kind: TypeKind,
    /// Variant names when [`TypeKind::Enum`]; empty otherwise.
    pub variants: Vec<String>,
    pub visibility: ItemVisibility,
}

/// A single binding produced by expanding a `use` / `pub use` tree.
///
/// Nested brace lists flatten to one [`Import`] per bound name (or glob).
/// Example: `use crate::text::{self, upper as shout}` yields two imports —
/// module path `crate::text` bound as `text`, and `crate::text::upper` bound
/// as `shout`.
///
/// `as _` produces no binding (discarded during extraction).
///
/// See the module-level note on function-body scope widening.
#[derive(Debug, Clone)]
pub struct Import {
    /// Target path with `crate` / `self` / `super` absolutized relative to
    /// [`module_path`] when those roots appear. External roots keep their
    /// crate name (`std::fs`). Glob imports store the module being globbed
    /// (no trailing `*`).
    pub path: String,
    /// Explicit `as` rename, if any. The local binding name is this when
    /// present, otherwise the last path segment (or the globbed module's
    /// last segment is unused — globs have [`is_glob`] set and no local name).
    pub alias: Option<String>,
    pub is_glob: bool,
    /// True when the `use` item has any `pub` visibility (a re-export).
    pub is_public: bool,
    /// Full visibility of the `use` item (controls who can see a re-export).
    pub visibility: ItemVisibility,
    /// Module containing the `use` (inline `mod` segments included).
    ///
    /// Function-body `use` items are attributed here too (module-wide), which
    /// is wider than rustc's real body scope — see the module docs.
    pub module_path: String,
}

impl Import {
    /// Local name bound in the importing module. `None` for globs.
    pub fn local_name(&self) -> Option<&str> {
        if self.is_glob {
            return None;
        }
        if let Some(alias) = &self.alias {
            return Some(alias.as_str());
        }
        self.path.rsplit("::").next()
    }
}

/// Where a pending call will attach after resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOwnerKind {
    /// Inside a free function — attach to that function.
    Function,
    /// Module-level (`const` / `static` init, …) — attach to the [`horizon_map::File`].
    File,
}

/// A call expression awaiting resolution (pre-map form).
///
/// Distinct from [`horizon_map::CallSite`], which is the post-resolution edge.
#[derive(Debug, Clone)]
pub struct PendingCall {
    /// Path text at the call site (e.g. `shapes::get`, `get`).
    pub call_path: String,
    /// 1-based line of the start of the call expression.
    pub line: u32,
    /// Byte offset (UTF-8) of the start of the call expression in the file.
    pub byte_start: u32,
    /// Byte offset (UTF-8) one past the end of the call expression.
    pub byte_end: u32,
    /// Enclosing free-function identity, if [`CallOwnerKind::Function`].
    pub enclosing_function: Option<FunctionId>,
    /// Module path of the call site (e.g. `crate::app`), used for relative
    /// and unqualified resolution. Never includes a function-name segment.
    pub module_path: String,
    pub owner: CallOwnerKind,
    /// Recovered from a macro argument token tree (see module docs).
    pub from_macro: bool,
}

/// Std/core macros whose arguments are expression positions we are willing to
/// open. Anything else stays opaque — prefer a missed call over a fabricated
/// edge (patterns in `matches!`, tokens in `stringify!`, templates in
/// `quote!`, cfg predicates, …).
const MACRO_EXPR_ALLOWLIST: &[&str] = &[
    "format",
    "print",
    "println",
    "eprint",
    "eprintln",
    "write",
    "writeln",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "vec",
    "dbg",
    "panic",
    "unreachable",
    "unimplemented",
    "todo",
];

/// Bound nested-macro recovery (`println!("{}", format!("{}", f()))`).
const MAX_MACRO_RECOVERY_DEPTH: usize = 8;

/// Whether `name` is an item-pasting macro whose token tree may be opened for
/// `mod` recovery during the module walk.
///
/// - `cfg_if` — the canonical crate; branches are unioned (same spirit as
///   keeping `#[cfg]`-twin function definitions).
/// - `cfg_*` / names starting with `cfg_` — Tokio-style feature-gate item
///   macros (`cfg_fs!`, `cfg_rt!`, `cfg_not_sync!`, …) that paste `$($item:item)*`.
///
/// Macros that do **not** evaluate their contents as items (`stringify!`,
/// `quote!`, `matches!`, user `macro_rules!` bodies, empty-arg expanders like
/// serde's `crate_root!()`) must stay closed. Prefer a missing subtree over a
/// fabricated module tree.
pub fn is_item_macro_allowlisted(name: &str) -> bool {
    name == "cfg_if" || name.starts_with("cfg_")
}

/// Synthetic callee used when re-parsing parenthesised macro args as a call.
const MACRO_PROBE_CALLEE: &str = "__horizon_macro_probe";

/// Extract definitions and call sites from `tree`.
///
/// `crate_key` is the [`FunctionId`] prefix for this compilation unit (see
/// [`horizon_map::Crate::function_id_prefix`]). `file_module_path` is the file's
/// module path from the `mod` walk (`crate`, `crate::shapes`, …). `source` is
/// the file text (for 1-based line mapping). `edition` is the crate edition
/// string (used when re-parsing macro token trees).
///
/// Function ids are provisional (no `#L` suffix yet); call
/// [`assign_function_ids`] over the whole crate before resolving.
pub fn extract_facts(
    tree: &SourceFile,
    source: &str,
    crate_key: &str,
    file_module_path: &str,
    edition: &str,
) -> Result<FileFacts> {
    let lines = LineIndex::new(source);
    let root = tree.syntax();

    let mut raw_defs = Vec::new();
    for node in root.descendants() {
        let Some(func) = ast::Fn::cast(node.clone()) else {
            continue;
        };
        if is_method_or_trait_item(func.syntax()) {
            continue;
        }
        let Some(name) = func.name() else {
            continue;
        };
        let name = name.text().to_string();
        let module_path = function_module_path(func.syntax(), &name, file_module_path);
        let line = func
            .fn_token()
            .map(|t| lines.line_of(u32::from(t.text_range().start())))
            .unwrap_or_else(|| lines.line_of(u32::from(func.syntax().text_range().start())));
        let visibility = visibility_of(&func);
        let doc_comments = extract_outer_docs(func.syntax());
        raw_defs.push(RawDef {
            name,
            module_path,
            line,
            visibility,
            doc_comments,
            syntax: func.syntax().clone(),
        });
    }

    // Provisional ids (no collision suffix); remapped crate-wide later.
    let mut functions = Vec::with_capacity(raw_defs.len());
    let mut function_visibility = Vec::with_capacity(raw_defs.len());
    for def in &raw_defs {
        functions.push(Function {
            id: FunctionId::from_parts(crate_key, &def.module_path, None),
            name: def.name.clone(),
            module_path: def.module_path.clone(),
            line: def.line,
            call_sites: Vec::new(),
            doc_comments: def.doc_comments.clone(),
        });
        function_visibility.push(def.visibility.clone());
    }

    let id_by_syntax: HashMap<SyntaxNode, FunctionId> = raw_defs
        .iter()
        .zip(functions.iter())
        .map(|(raw, func)| (raw.syntax.clone(), func.id.clone()))
        .collect();

    let types = extract_types(root, file_module_path);
    let imports = extract_imports(root, file_module_path);

    let mut call_sites = Vec::new();
    let mut seen_ranges: HashSet<(u32, u32)> = HashSet::new();
    for node in root.descendants() {
        let Some(call) = ast::CallExpr::cast(node.clone()) else {
            continue;
        };
        let Some(pending) = pending_from_call_expr(
            &call,
            &lines,
            file_module_path,
            &id_by_syntax,
            false,
        ) else {
            continue;
        };
        seen_ranges.insert((pending.byte_start, pending.byte_end));
        call_sites.push(pending);
    }

    {
        let mut ctx = MacroRecoveryCtx {
            source,
            edition,
            lines: &lines,
            file_module_path,
            id_by_syntax: &id_by_syntax,
            call_sites: &mut call_sites,
            seen_ranges: &mut seen_ranges,
        };
        recover_macro_calls(root, &mut ctx);
    }

    // Macro recovery appends; restore source order for the emitted map.
    call_sites.sort_by_key(|c| (c.byte_start, c.byte_end));

    Ok(FileFacts {
        functions,
        function_visibility,
        types,
        imports,
        call_sites,
        doc_comments: extract_inner_docs(root),
    })
}

/// Shared state for allowlisted macro token-tree recovery.
struct MacroRecoveryCtx<'a> {
    source: &'a str,
    edition: &'a str,
    lines: &'a LineIndex,
    file_module_path: &'a str,
    id_by_syntax: &'a HashMap<SyntaxNode, FunctionId>,
    call_sites: &'a mut Vec<PendingCall>,
    seen_ranges: &'a mut HashSet<(u32, u32)>,
}

fn pending_from_call_expr(
    call: &ast::CallExpr,
    lines: &LineIndex,
    file_module_path: &str,
    id_by_syntax: &HashMap<SyntaxNode, FunctionId>,
    from_macro: bool,
) -> Option<PendingCall> {
    let callee = call.expr()?;
    let call_path = squish(&callee.syntax().text().to_string());
    if !is_path_like_callee(&call_path) {
        return None;
    }
    if call_path == MACRO_PROBE_CALLEE {
        return None;
    }

    let range = call.syntax().text_range();
    let byte_start = u32::from(range.start());
    let byte_end = u32::from(range.end());

    match classify_call_owner(call.syntax()) {
        CallOwner::SkipImplOrTrait => None,
        CallOwner::FreeFunction(fn_node) => {
            let enclosing = id_by_syntax.get(&fn_node).cloned()?;
            Some(PendingCall {
                call_path,
                line: lines.line_of(byte_start),
                byte_start,
                byte_end,
                enclosing_function: Some(enclosing),
                module_path: call_module_path(call.syntax(), file_module_path),
                owner: CallOwnerKind::Function,
                from_macro,
            })
        }
        CallOwner::ModuleLevel => Some(PendingCall {
            call_path,
            line: lines.line_of(byte_start),
            byte_start,
            byte_end,
            enclosing_function: None,
            module_path: call_module_path(call.syntax(), file_module_path),
            owner: CallOwnerKind::File,
            from_macro,
        }),
    }
}

/// Open allowlisted `MacroCall` token trees and recover path-form calls.
fn recover_macro_calls(root: &SyntaxNode, ctx: &mut MacroRecoveryCtx<'_>) {
    for node in root.descendants() {
        let Some(mac) = ast::MacroCall::cast(node) else {
            continue;
        };
        if is_inside_macro_definition(mac.syntax()) {
            continue;
        }
        let Some(name) = macro_call_name(&mac) else {
            continue;
        };
        if !MACRO_EXPR_ALLOWLIST.contains(&name.as_str()) {
            continue;
        }
        let Some(tt) = mac.token_tree() else {
            continue;
        };
        let owner_node = mac.syntax();
        let Some(owner_ctx) =
            macro_owner_context(owner_node, ctx.file_module_path, ctx.id_by_syntax)
        else {
            continue;
        };
        let tt_range = tt.syntax().text_range();
        let content_start = u32::from(tt_range.start()).saturating_add(1);
        let content_end = u32::from(tt_range.end()).saturating_sub(1);
        if content_end < content_start || (content_end as usize) > ctx.source.len() {
            continue;
        }
        let open = ctx
            .source
            .as_bytes()
            .get(u32::from(tt_range.start()) as usize)
            .copied();
        let Some(open) = open else {
            continue;
        };
        let content = ctx.source[content_start as usize..content_end as usize].to_string();
        recover_from_macro_content(&content, open, content_start, &owner_ctx, ctx, 0);
    }
}

struct MacroOwnerContext {
    enclosing_function: Option<FunctionId>,
    module_path: String,
    owner: CallOwnerKind,
}

fn macro_owner_context(
    owner_node: &SyntaxNode,
    file_module_path: &str,
    id_by_syntax: &HashMap<SyntaxNode, FunctionId>,
) -> Option<MacroOwnerContext> {
    match classify_call_owner(owner_node) {
        CallOwner::SkipImplOrTrait => None,
        CallOwner::FreeFunction(fn_node) => {
            let enclosing = id_by_syntax.get(&fn_node).cloned()?;
            Some(MacroOwnerContext {
                enclosing_function: Some(enclosing),
                module_path: call_module_path(owner_node, file_module_path),
                owner: CallOwnerKind::Function,
            })
        }
        CallOwner::ModuleLevel => Some(MacroOwnerContext {
            enclosing_function: None,
            module_path: call_module_path(owner_node, file_module_path),
            owner: CallOwnerKind::File,
        }),
    }
}

pub(crate) fn macro_call_name(mac: &ast::MacroCall) -> Option<String> {
    let path = mac.path()?;
    let segs = path_segments(&path);
    segs.last().cloned()
}

pub(crate) fn is_inside_macro_definition(node: &SyntaxNode) -> bool {
    for ancestor in node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::MACRO_RULES | SyntaxKind::MACRO_DEF => return true,
            _ => {}
        }
    }
    false
}

/// Re-parse `content` (token-tree interior) and collect path-form [`CallExpr`]s,
/// mapping their ranges back into the original file via `content_start`.
fn recover_from_macro_content(
    content: &str,
    open_delim: u8,
    content_start: u32,
    owner_ctx: &MacroOwnerContext,
    ctx: &mut MacroRecoveryCtx<'_>,
    depth: usize,
) {
    if depth > MAX_MACRO_RECOVERY_DEPTH || content.is_empty() {
        return;
    }

    let (wrapped, wrapped_content_start) = match open_delim {
        b'(' => {
            let prefix = format!("fn __horizon_probe() {{ {MACRO_PROBE_CALLEE}(");
            let wrapped = format!("{prefix}{content}); }}");
            (wrapped, prefix.len() as u32)
        }
        b'[' => {
            let prefix = "fn __horizon_probe() { let _ = [";
            let wrapped = format!("{prefix}{content}]; }}");
            (wrapped, prefix.len() as u32)
        }
        b'{' => {
            let prefix = "fn __horizon_probe() { { ";
            let wrapped = format!("{prefix}{content} }} }}");
            (wrapped, prefix.len() as u32)
        }
        _ => return,
    };

    let Ok(tree) = crate::parse::parse_source(&wrapped, &ctx.edition.to_string()) else {
        return;
    };
    let root = tree.syntax();

    for node in root.descendants() {
        if let Some(call) = ast::CallExpr::cast(node.clone()) {
            let Some(callee) = call.expr() else {
                continue;
            };
            let call_path = squish(&callee.syntax().text().to_string());
            if call_path == MACRO_PROBE_CALLEE || !is_path_like_callee(&call_path) {
                continue;
            }
            let range = call.syntax().text_range();
            let wrapped_start = u32::from(range.start());
            let wrapped_end = u32::from(range.end());
            let Some(byte_start) =
                map_wrapped_offset(wrapped_start, wrapped_content_start, content_start)
            else {
                continue;
            };
            let Some(byte_end) =
                map_wrapped_offset(wrapped_end, wrapped_content_start, content_start)
            else {
                continue;
            };
            if byte_end < byte_start {
                continue;
            }
            // Require the mapped span to match the reparsed call text in the
            // original token-tree content — drops phantoms from a bad reparse.
            let call_text = call.syntax().text().to_string();
            let rel_start = (byte_start - content_start) as usize;
            let rel_end = (byte_end - content_start) as usize;
            let Some(orig_slice) = content.get(rel_start..rel_end) else {
                continue;
            };
            if orig_slice != call_text.as_str()
                && squish(orig_slice) != squish(&call_text)
            {
                continue;
            }
            if !ctx.seen_ranges.insert((byte_start, byte_end)) {
                continue;
            }
            ctx.call_sites.push(PendingCall {
                call_path,
                line: ctx.lines.line_of(byte_start),
                byte_start,
                byte_end,
                enclosing_function: owner_ctx.enclosing_function.clone(),
                module_path: owner_ctx.module_path.clone(),
                owner: owner_ctx.owner,
                from_macro: true,
            });
            continue;
        }

        if let Some(mac) = ast::MacroCall::cast(node) {
            let Some(name) = macro_call_name(&mac) else {
                continue;
            };
            if !MACRO_EXPR_ALLOWLIST.contains(&name.as_str()) {
                continue;
            }
            let Some(tt) = mac.token_tree() else {
                continue;
            };
            let tt_range = tt.syntax().text_range();
            let nested_wrapped_start = u32::from(tt_range.start()).saturating_add(1);
            let nested_wrapped_end = u32::from(tt_range.end()).saturating_sub(1);
            let Some(nested_orig_start) = map_wrapped_offset(
                nested_wrapped_start,
                wrapped_content_start,
                content_start,
            ) else {
                continue;
            };
            let Some(nested_orig_end) =
                map_wrapped_offset(nested_wrapped_end, wrapped_content_start, content_start)
            else {
                continue;
            };
            if nested_orig_end < nested_orig_start {
                continue;
            }
            let rel_s = (nested_orig_start - content_start) as usize;
            let rel_e = (nested_orig_end - content_start) as usize;
            let Some(nested_content) = content.get(rel_s..rel_e).map(str::to_string) else {
                continue;
            };
            let open = wrapped
                .as_bytes()
                .get(u32::from(tt_range.start()) as usize)
                .copied();
            let Some(open) = open else {
                continue;
            };
            recover_from_macro_content(
                &nested_content,
                open,
                nested_orig_start,
                owner_ctx,
                ctx,
                depth + 1,
            );
        }
    }
}

fn map_wrapped_offset(
    wrapped_offset: u32,
    wrapped_content_start: u32,
    original_content_start: u32,
) -> Option<u32> {
    if wrapped_offset < wrapped_content_start {
        return None;
    }
    Some(original_content_start + (wrapped_offset - wrapped_content_start))
}

/// Outer docs attached to an item (`///`, `/** … */`, `#[doc = "…"]`).
///
/// `ra_ap_syntax` keeps doc comments and attributes as children of the item,
/// so an intervening `#[inline]` between `///` and `fn` does not break the
/// association. Consecutive pieces are joined into one [`DocComment`].
fn extract_outer_docs(node: &SyntaxNode) -> Vec<DocComment> {
    let pieces = collect_doc_pieces(node, DocCommentKind::Outer);
    join_doc_pieces(DocCommentKind::Outer, pieces)
}

/// Inner docs for a file / module (`//!`, `/*! … */`, `#![doc = "…"]`).
fn extract_inner_docs(node: &SyntaxNode) -> Vec<DocComment> {
    let pieces = collect_doc_pieces(node, DocCommentKind::Inner);
    join_doc_pieces(DocCommentKind::Inner, pieces)
}

fn join_doc_pieces(kind: DocCommentKind, pieces: Vec<String>) -> Vec<DocComment> {
    if pieces.is_empty() {
        return Vec::new();
    }
    vec![DocComment {
        kind,
        text: pieces.join("\n"),
    }]
}

/// Walk direct children of `node` in source order, collecting doc comments and
/// `doc = "…"` attributes of the requested kind. Ordinary `//` / `/* */` are
/// skipped (`Comment::is_doc` is false for them).
fn collect_doc_pieces(node: &SyntaxNode, want: DocCommentKind) -> Vec<String> {
    let mut pieces = Vec::new();
    for element in node.children_with_tokens() {
        match element {
            SyntaxElement::Token(token) => {
                let Some(comment) = ast::Comment::cast(token) else {
                    continue;
                };
                if !comment.is_doc() {
                    continue;
                }
                let kind = if comment.is_inner() {
                    DocCommentKind::Inner
                } else {
                    DocCommentKind::Outer
                };
                if kind != want {
                    continue;
                }
                let Some((raw, _)) = comment.doc_comment() else {
                    continue;
                };
                pieces.push(normalize_doc_text(raw));
            }
            SyntaxElement::Node(child) => {
                let Some(attr) = ast::Attr::cast(child) else {
                    continue;
                };
                let kind = if attr.kind().is_inner() {
                    DocCommentKind::Inner
                } else {
                    DocCommentKind::Outer
                };
                if kind != want {
                    continue;
                }
                if let Some(raw) = doc_attr_string(&attr) {
                    pieces.push(normalize_doc_text(&raw));
                }
            }
        }
    }
    pieces
}

/// Body of `#[doc = "…"]` / `#![doc = "…"]`. Other `doc(…)` forms (e.g.
/// `#[doc(hidden)]`) are not documentation text and are ignored.
fn doc_attr_string(attr: &ast::Attr) -> Option<String> {
    let meta = attr.meta()?;
    let ast::Meta::KeyValueMeta(kv) = meta else {
        return None;
    };
    let path = kv.path()?;
    let name = path.as_single_name_ref()?;
    if name.text() != "doc" {
        return None;
    }
    let expr = kv.expr()?;
    let ast::Expr::Literal(lit) = expr else {
        return None;
    };
    match lit.kind() {
        LiteralKind::String(s) => s.value().ok().map(|cow| cow.into_owned()),
        _ => None,
    }
}

/// Strip the conventional single leading space after `///` / `//!` on each
/// line; keep further indentation (needed for fenced code blocks in docs).
fn normalize_doc_text(raw: &str) -> String {
    raw.lines()
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Collect struct / enum / trait / type-alias names (and enum variants).
///
/// Items inside `impl` / `trait` bodies are skipped — associated types are not
/// free type definitions for our purposes.
fn extract_types(root: &SyntaxNode, file_module_path: &str) -> Vec<TypeDef> {
    let mut types = Vec::new();
    for node in root.descendants() {
        if is_inside_impl_or_trait(&node) {
            continue;
        }
        if let Some(en) = ast::Enum::cast(node.clone()) {
            let Some(name) = en.name() else {
                continue;
            };
            let variants = en
                .variant_list()
                .map(|list| {
                    list.variants()
                        .filter_map(|v| v.name().map(|n| n.text().to_string()))
                        .collect()
                })
                .unwrap_or_default();
            types.push(TypeDef {
                name: name.text().to_string(),
                module_path: item_module_path(en.syntax(), file_module_path),
                kind: TypeKind::Enum,
                variants,
                visibility: visibility_of(&en),
            });
            continue;
        }
        if let Some(st) = ast::Struct::cast(node.clone()) {
            let Some(name) = st.name() else {
                continue;
            };
            types.push(TypeDef {
                name: name.text().to_string(),
                module_path: item_module_path(st.syntax(), file_module_path),
                kind: TypeKind::Struct,
                variants: Vec::new(),
                visibility: visibility_of(&st),
            });
            continue;
        }
        if let Some(tr) = ast::Trait::cast(node.clone()) {
            let Some(name) = tr.name() else {
                continue;
            };
            types.push(TypeDef {
                name: name.text().to_string(),
                module_path: item_module_path(tr.syntax(), file_module_path),
                kind: TypeKind::Trait,
                variants: Vec::new(),
                visibility: visibility_of(&tr),
            });
            continue;
        }
        if let Some(ta) = ast::TypeAlias::cast(node.clone()) {
            let Some(name) = ta.name() else {
                continue;
            };
            types.push(TypeDef {
                name: name.text().to_string(),
                module_path: item_module_path(ta.syntax(), file_module_path),
                kind: TypeKind::TypeAlias,
                variants: Vec::new(),
                visibility: visibility_of(&ta),
            });
        }
    }
    types
}

/// Expand every `use` / `pub use` tree and `extern crate` item into
/// flattened [`Import`]s.
///
/// `pub extern crate dep as name` is recorded like `pub use dep as name`: a
/// public binding of the dependency crate under `name`. That is the facade
/// form ripgrep's `grep` crate uses (`pub extern crate grep_cli as cli`).
fn extract_imports(root: &SyntaxNode, file_module_path: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for node in root.descendants() {
        if let Some(use_item) = ast::Use::cast(node.clone()) {
            // Nested `use` trees are walked from the top-level Use item only.
            // Descendant UseTree nodes must not be double-scanned as Use items.
            let Some(tree) = use_item.use_tree() else {
                continue;
            };
            let module_path = call_module_path(use_item.syntax(), file_module_path);
            let visibility = visibility_of(&use_item);
            let is_public = !matches!(visibility, ItemVisibility::Private);
            flatten_use_tree(
                &tree,
                &[],
                &module_path,
                &visibility,
                is_public,
                &mut imports,
            );
            continue;
        }
        let Some(ext) = ast::ExternCrate::cast(node.clone()) else {
            continue;
        };
        let Some(name_ref) = ext.name_ref() else {
            continue;
        };
        let crate_name = name_ref.text().to_string();
        let rename = ext.rename();
        if rename.as_ref().is_some_and(|r| r.underscore_token().is_some()) {
            continue;
        }
        let alias = rename.and_then(|r| r.name().map(|n| n.text().to_string()));
        let module_path = call_module_path(ext.syntax(), file_module_path);
        let visibility = visibility_of(&ext);
        let is_public = !matches!(visibility, ItemVisibility::Private);
        imports.push(Import {
            path: crate_name,
            alias,
            is_glob: false,
            is_public,
            visibility,
            module_path,
        });
    }
    imports
}

/// Recursively flatten a `UseTree` into leaf bindings.
///
/// `prefix` holds path segments accumulated from outer trees
/// (`use crate::{text::{upper}}` → prefix `[crate]` then `[crate, text]`).
fn flatten_use_tree(
    tree: &ast::UseTree,
    prefix: &[String],
    module_path: &str,
    visibility: &ItemVisibility,
    is_public: bool,
    out: &mut Vec<Import>,
) {
    let mut path_segs = prefix.to_vec();
    if let Some(path) = tree.path() {
        path_segs.extend(path_segments(&path));
    }

    if let Some(list) = tree.use_tree_list() {
        for child in list.use_trees() {
            flatten_use_tree(
                &child,
                &path_segs,
                module_path,
                visibility,
                is_public,
                out,
            );
        }
        return;
    }

    if tree.star_token().is_some() {
        let abs = absolutize_path_segments(&path_segs, module_path);
        out.push(Import {
            path: abs,
            alias: None,
            is_glob: true,
            is_public,
            visibility: visibility.clone(),
            module_path: module_path.to_string(),
        });
        return;
    }

    // `self` alone in a brace list: `use crate::text::{self}` — bind the
    // prefix module under its last segment name.
    if path_segs.last().is_some_and(|s| s == "self") && path_segs.len() > 1 {
        path_segs.pop();
    } else if path_segs.len() == 1 && path_segs[0] == "self" {
        // `use self::foo` keeps `self`; bare `{self}` at crate root is rare.
        // If the only segment is `self` with an empty prefix, bind the module.
        path_segs.clear();
        // Represent "this module" as the importing module path's segments.
        path_segs.extend(module_path_segments(module_path));
    }

    if path_segs.is_empty() {
        return;
    }

    let rename = tree.rename();
    if rename.as_ref().is_some_and(|r| r.underscore_token().is_some()) {
        // `as _` — intentionally binds nothing.
        return;
    }
    let alias = rename.and_then(|r| r.name().map(|n| n.text().to_string()));
    let abs = absolutize_path_segments(&path_segs, module_path);
    out.push(Import {
        path: abs,
        alias,
        is_glob: false,
        is_public,
        visibility: visibility.clone(),
        module_path: module_path.to_string(),
    });
}

fn path_segments(path: &ast::Path) -> Vec<String> {
    path.segments()
        .filter_map(|seg| match seg.kind()? {
            PathSegmentKind::Name(name) => Some(name.text().to_string()),
            PathSegmentKind::SelfKw => Some("self".into()),
            PathSegmentKind::SuperKw => Some("super".into()),
            PathSegmentKind::CrateKw => Some("crate".into()),
            PathSegmentKind::SelfTypeKw => Some("Self".into()),
            PathSegmentKind::Type { .. } => None,
        })
        .collect()
}

fn module_path_segments(module_path: &str) -> Vec<String> {
    module_path.split("::").map(str::to_string).collect()
}

/// Resolve leading `crate` / `self` / `super` relative to `from_module`.
///
/// Paths that start with any other segment (e.g. `std`, `serde`) are kept as
/// written — they name an external crate or an as-yet-unresolved root.
fn absolutize_path_segments(segments: &[String], from_module: &str) -> String {
    if segments.is_empty() {
        return from_module.to_string();
    }
    let mut module = from_module.to_string();
    let mut i = 0usize;
    let starts_with_kw = matches!(segments[0].as_str(), "crate" | "self" | "super");
    if starts_with_kw {
        while i < segments.len() {
            match segments[i].as_str() {
                "crate" => {
                    module = "crate".to_string();
                    i += 1;
                }
                "self" => {
                    i += 1;
                }
                "super" => {
                    match parent_module_path(&module) {
                        Some(p) => module = p,
                        None => break,
                    }
                    i += 1;
                }
                _ => break,
            }
        }
        let rest = &segments[i..];
        if rest.is_empty() {
            return module;
        }
        return join_path(&module, rest);
    }
    segments.join("::")
}

fn parent_module_path(module: &str) -> Option<String> {
    if module == "crate" {
        None
    } else {
        module.rsplit_once("::").map(|(p, _)| p.to_string())
    }
}

fn visibility_of(node: &impl HasVisibility) -> ItemVisibility {
    match node.visibility() {
        None => ItemVisibility::Private,
        Some(v) => match v.kind() {
            VisibilityKind::Pub => ItemVisibility::Public,
            VisibilityKind::PubCrate => ItemVisibility::Crate,
            VisibilityKind::PubSuper => ItemVisibility::Super,
            VisibilityKind::PubSelf => ItemVisibility::SelfMod,
            VisibilityKind::In(path) => {
                let segs = path_segments(&path);
                ItemVisibility::InPath(segs.join("::"))
            }
        },
    }
}

fn is_inside_impl_or_trait(node: &SyntaxNode) -> bool {
    for ancestor in node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::IMPL | SyntaxKind::TRAIT => return true,
            _ => {}
        }
    }
    false
}

/// Module path of a type / item (inline `mod` segments, no item-name segment).
fn item_module_path(node: &SyntaxNode, file_module_path: &str) -> String {
    call_module_path(node, file_module_path)
}

/// Assign [`FunctionId`]s after all definitions in a crate are known.
///
/// Colliding `module_path` values (typically `#[cfg]` duplicates) all receive
/// a `#L{line}` suffix. Updates `functions` in place and returns a map from
/// provisional id → final id so pending calls can be remapped.
pub fn assign_function_ids(
    crate_key: &str,
    functions: &mut [Function],
) -> HashMap<FunctionId, FunctionId> {
    let mut path_counts: HashMap<String, usize> = HashMap::new();
    for func in functions.iter() {
        *path_counts
            .entry(func.module_path.clone())
            .or_default() += 1;
    }

    let mut remap = HashMap::new();
    for func in functions.iter_mut() {
        let old = func.id.clone();
        let line = if path_counts.get(&func.module_path).copied().unwrap_or(0) > 1 {
            Some(func.line)
        } else {
            None
        };
        let new_id = FunctionId::from_parts(crate_key, &func.module_path, line);
        remap.insert(old, new_id.clone());
        func.id = new_id;
    }
    remap
}

/// Remap [`PendingCall::enclosing_function`] after crate-wide id assignment.
pub fn remap_pending_calls(calls: &mut [PendingCall], remap: &HashMap<FunctionId, FunctionId>) {
    for call in calls {
        if let Some(old) = call.enclosing_function.take() {
            call.enclosing_function = Some(remap.get(&old).cloned().unwrap_or(old));
        }
    }
}

struct RawDef {
    name: String,
    module_path: String,
    line: u32,
    visibility: ItemVisibility,
    doc_comments: Vec<DocComment>,
    syntax: SyntaxNode,
}

enum CallOwner {
    FreeFunction(SyntaxNode),
    ModuleLevel,
    SkipImplOrTrait,
}

fn classify_call_owner(node: &SyntaxNode) -> CallOwner {
    for ancestor in node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::FN => {
                if is_method_or_trait_item(&ancestor) {
                    return CallOwner::SkipImplOrTrait;
                }
                return CallOwner::FreeFunction(ancestor);
            }
            SyntaxKind::IMPL | SyntaxKind::TRAIT => {
                return CallOwner::SkipImplOrTrait;
            }
            _ => {}
        }
    }
    CallOwner::ModuleLevel
}

/// Full module path including the function name, rooted at `crate`.
///
/// Starts from the file's module path (from the `mod` walk), then adds inline
/// `mod` segments and enclosing free-function names for nested `fn`s.
fn function_module_path(fn_node: &SyntaxNode, name: &str, file_module_path: &str) -> String {
    let mut parts = Vec::new();
    for ancestor in fn_node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::MODULE => {
                if let Some(n) = ast::Module::cast(ancestor).and_then(|m| m.name()) {
                    parts.push(n.text().to_string());
                }
            }
            SyntaxKind::FN => {
                if let Some(n) = ast::Fn::cast(ancestor).and_then(|f| f.name()) {
                    parts.push(n.text().to_string());
                }
            }
            _ => {}
        }
    }
    parts.reverse();
    parts.push(name.to_string());
    join_path(file_module_path, &parts)
}

/// Module path of a call site (no function-name segment).
fn call_module_path(node: &SyntaxNode, file_module_path: &str) -> String {
    let mut parts = Vec::new();
    for ancestor in node.ancestors().skip(1) {
        if ancestor.kind() == SyntaxKind::MODULE {
            if let Some(n) = ast::Module::cast(ancestor).and_then(|m| m.name()) {
                parts.push(n.text().to_string());
            }
        }
    }
    parts.reverse();
    if parts.is_empty() {
        file_module_path.to_string()
    } else {
        join_path(file_module_path, &parts)
    }
}

fn join_path(file_module_path: &str, parts: &[String]) -> String {
    if parts.is_empty() {
        return file_module_path.to_string();
    }
    if file_module_path == "crate" {
        format!("crate::{}", parts.join("::"))
    } else {
        format!("{file_module_path}::{}", parts.join("::"))
    }
}

fn is_method_or_trait_item(fn_node: &SyntaxNode) -> bool {
    for ancestor in fn_node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::IMPL | SyntaxKind::TRAIT => return true,
            SyntaxKind::FN => return false,
            _ => {}
        }
    }
    false
}

fn is_path_like_callee(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    path.split("::").all(|seg| {
        let seg = seg.split('<').next().unwrap_or(seg);
        is_ident(seg) || matches!(seg, "crate" | "self" | "super")
    })
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    }
}

fn squish(s: &str) -> String {
    s.split_whitespace().collect()
}

struct LineIndex {
    /// Byte offset of the start of each 1-based line (`starts[0] == 0`).
    starts: Vec<u32>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push((i + 1) as u32);
            }
        }
        Self { starts }
    }

    fn line_of(&self, offset: u32) -> u32 {
        let idx = self.starts.partition_point(|&s| s <= offset);
        idx as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_source;

    #[test]
    fn assigns_line_suffix_only_on_collision() {
        let source = r#"
fn unique() {}

#[cfg(unix)]
fn open() {}

#[cfg(windows)]
fn open() {}
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let mut facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        assign_function_ids("demo", &mut facts.functions);
        let ids: Vec<_> = facts.functions.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"demo::unique"));
        assert!(ids.iter().any(|id| id.starts_with("demo::open#L")));
        assert_eq!(
            ids.iter()
                .filter(|id| id.starts_with("demo::open#L"))
                .count(),
            2
        );
    }

    #[test]
    fn file_module_path_prefixes_definitions() {
        let source = r#"
pub fn get() {}
mod inner {
    pub fn helper() {}
}
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate::shapes", "2021").unwrap();
        let paths: Vec<_> = facts.functions.iter().map(|f| f.module_path.as_str()).collect();
        assert!(paths.contains(&"crate::shapes::get"));
        assert!(paths.contains(&"crate::shapes::inner::helper"));
    }

    #[test]
    fn module_level_const_call_is_file_owned() {
        let source = r#"
const fn compute_max() -> usize { 64 }
const MAX: usize = compute_max();
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        let file_calls: Vec<_> = facts
            .call_sites
            .iter()
            .filter(|c| c.owner == CallOwnerKind::File)
            .collect();
        assert_eq!(file_calls.len(), 1);
        assert_eq!(file_calls[0].call_path, "compute_max");
        assert!(file_calls[0].enclosing_function.is_none());
    }

    #[test]
    fn extracts_enums_structs_and_variants() {
        let source = r#"
pub enum Target {
    Resolved,
    Conflict,
}
pub struct Id(pub u32);
pub trait Marker {}
pub type Alias = Id;
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        assert_eq!(facts.types.len(), 4);
        let en = facts.types.iter().find(|t| t.name == "Target").unwrap();
        assert_eq!(en.kind, TypeKind::Enum);
        assert_eq!(en.variants, vec!["Resolved", "Conflict"]);
        assert_eq!(en.visibility, ItemVisibility::Public);
        assert!(facts.types.iter().any(|t| t.name == "Id" && t.kind == TypeKind::Struct));
        assert!(facts.types.iter().any(|t| t.name == "Marker" && t.kind == TypeKind::Trait));
        assert!(facts
            .types
            .iter()
            .any(|t| t.name == "Alias" && t.kind == TypeKind::TypeAlias));
    }

    #[test]
    fn flattens_nested_use_trees_and_self() {
        let source = r#"
use crate::{
    alpha::{one, two as second},
    beta::three,
};
use crate::alpha::{self, four};
use crate::beta::Marker as _;
use crate::shapes::*;
use std::fs;
pub use numbers::mean;
pub use text::upper as shout_upper;
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();

        let by_local = |name: &str| {
            facts
                .imports
                .iter()
                .find(|i| !i.is_glob && i.local_name() == Some(name))
                .unwrap_or_else(|| panic!("missing import {name}"))
        };

        assert_eq!(by_local("one").path, "crate::alpha::one");
        assert_eq!(by_local("second").path, "crate::alpha::two");
        assert_eq!(by_local("three").path, "crate::beta::three");
        assert_eq!(by_local("alpha").path, "crate::alpha");
        assert_eq!(by_local("four").path, "crate::alpha::four");
        // Bare paths stay bare until ResolveIndex::build normalizes them
        // against the module tree / extern crate set (edition 2018+ rules).
        assert_eq!(by_local("mean").path, "numbers::mean");
        assert!(by_local("mean").is_public);
        assert_eq!(by_local("shout_upper").path, "text::upper");
        assert_eq!(by_local("fs").path, "std::fs");

        assert!(facts.imports.iter().any(|i| i.is_glob && i.path == "crate::shapes"));
        assert!(
            !facts.imports.iter().any(|i| i.path.contains("Marker")),
            "`as _` must bind nothing"
        );
    }

    #[test]
    fn function_body_use_is_attributed_to_enclosing_module() {
        // Known limitation: body-scoped `use` is recorded as module-wide.
        let source = r#"
fn f() {
    use crate::helper;
    helper();
}
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        let imp = facts
            .imports
            .iter()
            .find(|i| i.local_name() == Some("helper"))
            .expect("helper import");
        assert_eq!(imp.module_path, "crate");
    }

    #[test]
    fn extracts_doc_comments_with_pinned_whitespace() {
        use horizon_map::DocCommentKind;

        let source = r#"//! File-level module docs.
//! Second inner line.

/// Single-line outer.
pub fn alpha() {}

/// First outer line.
/// Second outer line.
pub fn beta() {}

/// Docs above attribute.
#[inline]
pub fn gamma() {}

/**Block outer docs.*/
pub fn delta() {}

// Ordinary comment — must not be collected.
pub fn epsilon() {}

#[doc = "Attr docs."]
pub fn zeta() {}
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();

        assert_eq!(facts.doc_comments.len(), 1);
        assert_eq!(facts.doc_comments[0].kind, DocCommentKind::Inner);
        assert_eq!(
            facts.doc_comments[0].text,
            "File-level module docs.\nSecond inner line."
        );

        let by_name = |name: &str| {
            facts
                .functions
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };

        assert_eq!(by_name("alpha").doc_comments.len(), 1);
        assert_eq!(by_name("alpha").doc_comments[0].kind, DocCommentKind::Outer);
        assert_eq!(by_name("alpha").doc_comments[0].text, "Single-line outer.");

        assert_eq!(
            by_name("beta").doc_comments[0].text,
            "First outer line.\nSecond outer line."
        );
        assert_eq!(
            by_name("gamma").doc_comments[0].text,
            "Docs above attribute."
        );
        assert_eq!(by_name("delta").doc_comments[0].text, "Block outer docs.");
        assert!(
            by_name("epsilon").doc_comments.is_empty(),
            "ordinary // must not become a DocComment"
        );
        assert_eq!(by_name("zeta").doc_comments[0].text, "Attr docs.");
    }

    #[test]
    fn recovers_calls_inside_allowlisted_macros_only() {
        let source = r#"
fn mean(v: i32) -> i32 { v }
fn helper() -> i32 { 1 }
struct Foo(i32);
fn run() {
    let _ = format!("{}", mean(1));
    let _ = matches!(Some(1), Some(_y));
    let _ = stringify!(helper());
    let _ = vec![Foo(1), helper()];
    let _ = println!("{}", format!("{}", helper()));
}
macro_rules! m {
    ($name:ident($a:expr)) => { $name($a) };
}
fn through_macro() { m!(mean(1)); }
"#;
        let tree = parse_source(source, &"2021".into()).unwrap();
        let facts = extract_facts(&tree, source, "demo", "crate", "2021").unwrap();
        let run_id = facts
            .functions
            .iter()
            .find(|f| f.name == "run")
            .unwrap()
            .id
            .clone();
        let in_run: Vec<_> = facts
            .call_sites
            .iter()
            .filter(|c| c.enclosing_function.as_ref() == Some(&run_id))
            .collect();
        let paths: Vec<&str> = in_run.iter().map(|c| c.call_path.as_str()).collect();
        assert!(paths.contains(&"mean"), "format! should recover mean: {paths:?}");
        assert!(
            paths.iter().filter(|p| **p == "helper").count() >= 2,
            "vec! + nested format! should recover helper: {paths:?}"
        );
        assert!(
            in_run
                .iter()
                .filter(|c| c.call_path == "mean" || c.call_path == "helper")
                .all(|c| c.from_macro),
            "recovered mean/helper sites must set from_macro"
        );
        assert!(
            !paths.contains(&"Some"),
            "matches! must not recover pattern constructors: {paths:?}"
        );
        // `Foo(1)` may appear as a pending CallExpr; resolve drops constructors.

        let through = facts
            .functions
            .iter()
            .find(|f| f.name == "through_macro")
            .unwrap()
            .id
            .clone();
        assert!(
            !facts
                .call_sites
                .iter()
                .any(|c| c.enclosing_function.as_ref() == Some(&through)),
            "user macros / macro_rules bodies must stay closed"
        );
    }
}
